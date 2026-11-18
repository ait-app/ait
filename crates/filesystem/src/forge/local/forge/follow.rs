//! One read of a pull request by number for Bot tickets that follow it (ADR-287): state,
//! checks of the head and the newest review activity, with entries the CLI's account wrote
//! marked.

use std::path::Path;

use chrono::{DateTime, Utc};
use domain::forge::{
    AuthorAccess, CheckStatus, PullCheck, PullItem, PullItemKind, PullSnapshot, PullState,
    ReviewState,
};
use serde_json::Value;

use super::{
    ForgeContext, ForgeFailureKind, ForgeRuntimeError, LocalForge, READ_TIMEOUT, forge_error,
    optional_string, parse_checks, parse_json,
};
use crate::forge::ports::PullRequestCheck;

/// Newest entries only: the follower cares about what is new, and every count stays below the
/// bound of handled ids a binding keeps (30 + 50 + 30 x 10 = 380 < 512). Checks are read up to
/// [`domain::forge::CHECK_LIMIT`] (100), the bound GitLab reads use too. Each entry carries its
/// author's association with the repository and the author's type, so only entries by people
/// who can write to it reach a worker; review comments carry their review's state, so a
/// pending review's drafts are left out (ADR-287).
const FOLLOW_QUERY: &str = r"
query PullRequestFollow($owner: String!, $name: String!, $number: Int!) {
  viewer { login }
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      number url title state mergedAt headRefOid
      commits(last: 1) { nodes { commit { statusCheckRollup { contexts(first: 100) { nodes {
        __typename
        ... on CheckRun { name status conclusion detailsUrl databaseId }
        ... on StatusContext { context state targetUrl }
      } } } } } }
      reviews(last: 30) {
        nodes {
          id state body url submittedAt viewerDidAuthor authorAssociation
          author { __typename login }
        }
      }
      comments(last: 50) {
        nodes {
          id body url createdAt publishedAt viewerDidAuthor authorAssociation
          author { __typename login }
        }
      }
      reviewThreads(last: 30) {
        nodes {
          path line
          comments(last: 10) {
            nodes {
              id body url createdAt publishedAt viewerDidAuthor authorAssociation
              author { __typename login }
              pullRequestReview { state }
            }
          }
        }
      }
    }
  }
}";
/// Dates of a conversation comment, preferred first.
const COMMENT_DATES: &[&str] = &["publishedAt", "createdAt"];
/// Date of a review comment: when its review was published, which can be later than when it
/// was drafted; a comment of an unsubmitted review has none and is left out.
const REVIEW_COMMENT_DATES: &[&str] = &["publishedAt"];
/// Date of a review.
const REVIEW_DATES: &[&str] = &["submittedAt"];
/// GitHub author associations whose write access a permission lookup must confirm: an
/// organization member or a collaborator may hold only read or triage access.
const UNCONFIRMED: &[&str] = &["MEMBER", "COLLABORATOR"];
/// GitHub repository roles that can write to it.
const WRITE_ROLES: &[&str] = &["admin", "maintain", "write"];

/// Read GitHub pull request `number` of `context`'s repository through `forge` in `cwd`.
///
/// # Errors
/// Returns `NotFound` when the repository has no such pull request or GitHub cannot resolve
/// the repository itself (deleted, or no longer visible to the host's account), and CLI,
/// authentication or malformed-response failures.
pub(super) fn github(
    forge: &LocalForge,
    cwd: &Path,
    context: &ForgeContext,
    number: u64,
) -> Result<PullSnapshot, ForgeRuntimeError> {
    let (owner, name) = context
        .project_path
        .rsplit_once('/')
        .ok_or_else(|| forge_error(ForgeFailureKind::NoRemote, "Unsupported GitHub remote"))?;
    let arguments = vec![
        "api".to_owned(),
        "graphql".to_owned(),
        "-f".to_owned(),
        format!("query={FOLLOW_QUERY}"),
        "-f".to_owned(),
        format!("owner={owner}"),
        "-f".to_owned(),
        format!("name={name}"),
        "-F".to_owned(),
        format!("number={number}"),
    ];
    let output = forge
        .gh(cwd, context, &arguments, READ_TIMEOUT)
        .map_err(|mut error| {
            // `gh` exits 1 when GraphQL cannot resolve the repository: it was deleted, or the
            // host's account can no longer see it. For a follow that is the pull request gone.
            if error
                .message
                .to_ascii_lowercase()
                .contains("could not resolve to a repository")
            {
                error.kind = ForgeFailureKind::NotFound;
            }
            error
        })?;
    parse_github(&parse_json(&output)?, number)
}

/// Project the GraphQL `value` of pull request `number`.
///
/// # Errors
/// Returns `NotFound` for a missing pull request, the forge's own failure when the response
/// has errors and no pull request (such as a rate limit), and malformed-response failures.
fn parse_github(value: &Value, number: u64) -> Result<PullSnapshot, ForgeRuntimeError> {
    let pr = value
        .pointer("/data/repository/pullRequest")
        .filter(|pr| !pr.is_null())
        .ok_or_else(|| missing(value, number))?;
    let state = match pr.get("state").and_then(Value::as_str) {
        _ if optional_string(pr, "mergedAt").is_some() => PullState::Merged,
        Some("MERGED") => PullState::Merged,
        Some("CLOSED") => PullState::Closed,
        _ => PullState::Open,
    };
    let rollup = pr.pointer("/commits/nodes/0/commit/statusCheckRollup");
    let mut items = Vec::new();
    for review in nodes(pr, "reviews") {
        let kind = match review.get("state").and_then(Value::as_str) {
            Some("APPROVED") => ReviewState::Approved,
            Some("CHANGES_REQUESTED") => ReviewState::ChangesRequested,
            Some("COMMENTED") => ReviewState::Commented,
            _ => continue,
        };
        items.extend(item(review, PullItemKind::Review(kind), REVIEW_DATES, None));
    }
    for comment in nodes(pr, "comments") {
        items.extend(item(comment, PullItemKind::Comment, COMMENT_DATES, None));
    }
    for thread in nodes(pr, "reviewThreads") {
        let location = (
            optional_string(thread, "path"),
            thread.get("line").and_then(Value::as_u64),
        );
        for comment in nodes(thread, "comments").iter().filter(|comment| {
            comment
                .pointer("/pullRequestReview/state")
                .and_then(Value::as_str)
                != Some("PENDING")
        }) {
            items.extend(item(
                comment,
                PullItemKind::ReviewComment,
                REVIEW_COMMENT_DATES,
                Some(location.clone()),
            ));
        }
    }
    Ok(PullSnapshot {
        number: pr.get("number").and_then(Value::as_u64).unwrap_or(number),
        url: optional_string(pr, "url").unwrap_or_default(),
        title: optional_string(pr, "title").unwrap_or_default(),
        state,
        head: optional_string(pr, "headRefOid"),
        checks: parse_checks(rollup).into_iter().map(check).collect(),
        items,
    })
}

/// The neutral check of a projected forge check.
pub(super) fn check(check: PullRequestCheck) -> PullCheck {
    PullCheck {
        status: CheckStatus::from_forge(check.status),
        run: check.check_run_id.map(|id| id.to_string()),
        name: check.name,
        url: check.url,
    }
}

/// The failure for a response without pull request `number`: `NotFound` when the pull
/// request is missing, otherwise the first GraphQL error, such as a rate limit.
fn missing(value: &Value, number: u64) -> ForgeRuntimeError {
    let errors = value
        .get("errors")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    match errors
        .iter()
        .find(|error| error.get("type").and_then(Value::as_str) != Some("NOT_FOUND"))
    {
        Some(error) => forge_error(
            ForgeFailureKind::Unknown,
            format!(
                "Pull request #{number} could not be read: {}",
                optional_string(error, "message").unwrap_or_default()
            ),
        ),
        None => forge_error(
            ForgeFailureKind::NotFound,
            format!("Pull request #{number} not found"),
        ),
    }
}

/// The neutral entry of one GraphQL review or comment `value`, dated by the first of its
/// `dates` fields it has; `None` without an id or a date.
fn item(
    value: &Value,
    kind: PullItemKind,
    dates: &[&str],
    location: Option<(Option<String>, Option<u64>)>,
) -> Option<PullItem> {
    let (path, line) = location.unwrap_or_default();
    let created_at = dates
        .iter()
        .find_map(|date| optional_string(value, date))
        .and_then(|date| DateTime::parse_from_rfc3339(&date).ok())?
        .with_timezone(&Utc);
    Some(PullItem {
        id: optional_string(value, "id")?,
        kind,
        author: value
            .pointer("/author/login")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_owned(),
        own: value.get("viewerDidAuthor").and_then(Value::as_bool) == Some(true),
        access: github_access(value),
        body: optional_string(value, "body").unwrap_or_default(),
        url: optional_string(value, "url").unwrap_or_default(),
        created_at,
        path,
        line,
    })
}

/// What the author of GraphQL entry `value` may do: an app (`Bot`) account, the repository's
/// owner, an organization member or collaborator whose write access is still to be looked
/// up, or anyone else, including a deleted account.
fn github_access(value: &Value) -> AuthorAccess {
    if value.pointer("/author/__typename").and_then(Value::as_str) == Some("Bot") {
        return AuthorAccess::Bot;
    }
    match value.get("authorAssociation").and_then(Value::as_str) {
        Some("OWNER") => AuthorAccess::Writer,
        Some(association) if UNCONFIRMED.contains(&association) => AuthorAccess::Unconfirmed,
        _ => AuthorAccess::Other,
    }
}

/// Whether GitHub user `login` can write to `context`'s repository, read through `forge` in
/// `cwd` from `repos/{owner}/{repo}/collaborators/{login}/permission`: its role is admin,
/// maintain or write. A login GitHub does not know there (HTTP 404), or one that is not a valid
/// GitHub login, cannot. Neither can one GitHub refuses to tell about with HTTP 403, other
/// than a rate limit: the host's account has no push access (for example it pushed the branch
/// to a fork) or uses an app token, which a later lookup does not change, so Ait cannot
/// confirm the author can write (ADR-287).
///
/// # Errors
/// Returns CLI, authentication or forge failures, such as a rate limit.
pub(super) fn github_can_write(
    forge: &LocalForge,
    cwd: &Path,
    context: &ForgeContext,
    login: &str,
) -> Result<bool, ForgeRuntimeError> {
    if login.is_empty()
        || !login
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        return Ok(false);
    }
    let arguments = vec![
        "api".to_owned(),
        format!(
            "repos/{}/collaborators/{login}/permission",
            context.project_path
        ),
    ];
    match forge.gh(cwd, context, &arguments, READ_TIMEOUT) {
        Ok(output) => Ok(writes(&parse_json(&output)?)),
        Err(error) if cannot_tell(&error.message) => Ok(false),
        Err(error) => Err(error),
    }
}

/// Whether permission lookup failure `message` is GitHub's lasting answer that the author's
/// access cannot be told: the login is unknown (HTTP 404), or the request is refused (HTTP 403)
/// for a reason other than a rate limit, which passes.
fn cannot_tell(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("http 404")
        || (message.contains("http 403") && !message.contains("rate limit"))
}

/// Whether the permission response `value` grants write access: its `role_name` (which tells
/// maintain apart) or its `permission` is a writing role.
fn writes(value: &Value) -> bool {
    ["role_name", "permission"].iter().any(|field| {
        value
            .get(field)
            .and_then(Value::as_str)
            .is_some_and(|role| WRITE_ROLES.contains(&role))
    })
}

fn nodes<'a>(value: &'a Value, field: &str) -> &'a [Value] {
    value
        .get(field)
        .and_then(|value| value.get("nodes"))
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

#[cfg(test)]
mod tests;
