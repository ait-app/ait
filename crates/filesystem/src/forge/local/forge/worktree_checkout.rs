//! Forge checkout facts, without mutating the source checkout.

use super::{ForgeContext, ForgeKind, LocalForge, READ_TIMEOUT, Value, require_git_directory};
use crate::worktrees::ports::worktrees::{
    ChangeRequestCheckout, ChangeRequestCheckoutRef, ChangeRequestResolver, WorktreeError,
};
use domain::workspace::worktrees::WorktreeChangeRequest;

const CHECKOUT_QUERY: &str = "query PullRequestCheckoutTarget($owner: String!, $name: String!, $number: Int!) { repository(owner: $owner, name: $name) { pullRequest(number: $number) { number baseRefName headRefName isCrossRepository headRepositoryOwner { login } headRepository { sshUrl url } } } }";

impl ChangeRequestResolver for LocalForge {
    /// Resolve a selected GitHub/GitLab request through the existing authenticated CLI.
    /// # Errors
    /// Rejects unsupported forges, invalid identities, unavailable CLI, or malformed forge data.
    fn resolve_change_request(
        &self,
        cwd: &str,
        source: &WorktreeChangeRequest,
        head_ref: Option<&str>,
    ) -> Result<ChangeRequestCheckout, WorktreeError> {
        if source
            .forge
            .as_deref()
            .is_some_and(|forge| !matches!(forge, "github" | "gitlab"))
        {
            return Err(WorktreeError::ForgeUnavailable);
        }
        if source.number == 0 || source.number > 9_007_199_254_740_991 {
            return Err(WorktreeError::Invalid(
                "Invalid pull request number".to_owned(),
            ));
        }
        let cwd = require_git_directory(cwd).map_err(|_| lookup_failed())?;
        let context = match self.forge_context(&cwd) {
            Ok(context) => context,
            Err(error)
                if error.kind == super::ForgeFailureKind::NoRemote
                    && source.forge.as_deref() == Some("github") =>
            {
                ForgeContext {
                    kind: ForgeKind::Github,
                    host: "github.com".to_owned(),
                    project_path: String::new(),
                }
            }
            Err(_) => return Err(lookup_failed()),
        };
        let name = match context.kind {
            ForgeKind::Github => "github",
            ForgeKind::Gitlab => "gitlab",
        };
        if source.forge.as_deref().is_some_and(|forge| forge != name) {
            return Err(WorktreeError::ForgeUnavailable);
        }
        if context.kind == ForgeKind::Gitlab {
            return super::gitlab::checkout(self, &cwd, &context, source.number, head_ref);
        }
        self.github_checkout(&cwd, &context, source.number, head_ref)
    }
}

impl LocalForge {
    fn github_checkout(
        &self,
        cwd: &std::path::Path,
        context: &ForgeContext,
        number: u64,
        head_ref: Option<&str>,
    ) -> Result<ChangeRequestCheckout, WorktreeError> {
        let repository = self
            .gh(
                cwd,
                context,
                &[
                    "repo".into(),
                    "view".into(),
                    "--json".into(),
                    "owner,name".into(),
                ],
                READ_TIMEOUT,
            )
            .map_err(|_| lookup_failed())?;
        let repository: Value = serde_json::from_str(&repository).map_err(|_| lookup_failed())?;
        let owner = repository["owner"]["login"]
            .as_str()
            .ok_or_else(lookup_failed)?;
        let name = repository["name"].as_str().ok_or_else(lookup_failed)?;
        let response = self
            .gh(
                cwd,
                context,
                &[
                    "api".into(),
                    "graphql".into(),
                    "-f".into(),
                    format!("query={CHECKOUT_QUERY}"),
                    "-F".into(),
                    format!("owner={owner}"),
                    "-F".into(),
                    format!("name={name}"),
                    "-F".into(),
                    format!("number={number}"),
                ],
                READ_TIMEOUT,
            )
            .map_err(|_| lookup_failed())?;
        let response: Value = serde_json::from_str(&response).map_err(|_| lookup_failed())?;
        let facts = &response["data"]["repository"]["pullRequest"];
        if facts["number"].as_u64() != Some(number) {
            return Err(lookup_failed());
        }
        let mut target = parse_target(facts, number, head_ref)?;
        if target.head_ref.is_empty() {
            let response = self
                .gh(
                    cwd,
                    context,
                    &[
                        "pr".into(),
                        "view".into(),
                        number.to_string(),
                        "--json".into(),
                        "headRefName".into(),
                    ],
                    READ_TIMEOUT,
                )
                .map_err(|_| lookup_failed())?;
            let response: Value = serde_json::from_str(&response).map_err(|_| lookup_failed())?;
            let head = response["headRefName"]
                .as_str()
                .ok_or_else(lookup_failed)?
                .to_owned();
            target = parse_target(facts, number, Some(&head))?;
        }
        Ok(target)
    }
}

fn parse_target(
    value: &Value,
    number: u64,
    head_ref: Option<&str>,
) -> Result<ChangeRequestCheckout, WorktreeError> {
    let cross = value["isCrossRepository"].as_bool().unwrap_or(false);
    let owner = value["headRepositoryOwner"]["login"]
        .as_str()
        .map(str::trim)
        .unwrap_or_default();
    let head = head_ref
        .map(str::trim)
        .filter(|head| !head.is_empty())
        .or_else(|| value["headRefName"].as_str())
        .unwrap_or_default()
        .trim()
        .to_owned();
    let url = value["headRepository"]["url"]
        .as_str()
        .filter(|url| !url.is_empty());
    let ssh = value["headRepository"]["sshUrl"]
        .as_str()
        .filter(|url| !url.is_empty());
    let push_remote_url = cross.then(|| ssh.or(url)).flatten().map(str::to_owned);
    if push_remote_url
        .as_deref()
        .is_some_and(|url| !safe_remote(url))
    {
        return Err(lookup_failed());
    }
    let normalized_owner = owner.to_ascii_lowercase();
    let local_branch = if cross
        && !normalized_owner.is_empty()
        && normalized_owner
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        format!("{normalized_owner}/{head}")
    } else {
        head.clone()
    };
    Ok(ChangeRequestCheckout {
        forge: "github".to_owned(),
        number,
        head_ref: head,
        base_ref: value["baseRefName"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_owned(),
        local_branch,
        track_origin: !cross,
        push_remote_url,
        untrusted_repository: cross.then(|| repository_identity(owner, url.or(ssh))),
        checkout_refs: ["origin", "upstream"]
            .into_iter()
            .map(|remote| ChangeRequestCheckoutRef {
                remote: remote.to_owned(),
                reference: format!("refs/pull/{number}/head"),
            })
            .collect(),
    })
}

fn repository_identity(owner: &str, url: Option<&str>) -> String {
    let parts: Vec<_> = url
        .unwrap_or_default()
        .trim_end_matches(".git")
        .split(['/', ':'])
        .filter(|part| !part.is_empty())
        .collect();
    let repository = parts.last().copied();
    let owner = (!owner.is_empty())
        .then_some(owner)
        .or_else(|| parts.len().checked_sub(2).map(|index| parts[index]));
    match (owner, repository) {
        (Some(owner), Some(repository)) => format!("{owner}/{repository}"),
        (Some(owner), None) => owner.to_owned(),
        (None, Some(repository)) => repository.to_owned(),
        (None, None) => "unknown repository".to_owned(),
    }
}

fn safe_remote(url: &str) -> bool {
    (url.starts_with("https://")
        || url.starts_with("http://")
        || url.starts_with("ssh://")
        || url.starts_with("git@"))
        && !url.chars().any(char::is_control)
}

fn lookup_failed() -> WorktreeError {
    WorktreeError::Io("Unable to resolve pull request checkout through GitHub CLI".to_owned())
}

#[cfg(test)]
mod tests;
