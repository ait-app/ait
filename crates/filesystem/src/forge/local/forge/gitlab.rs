//! GitLab operations using the same glab commands and projections as Paseo.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde_json::Value;

use super::{
    CommandFamily, ForgeAuthState, ForgeContext, ForgeFailureKind, ForgeRuntimeError, ForgeSearch,
    ForgeSearchItem, ForgeSearchKind, LocalForge, PullRequestCreated, PullRequestMergeMethod,
    PullRequestStatusRead, READ_TIMEOUT, WRITE_TIMEOUT, current_number, forge_error, git_optional,
    integer, malformed, optional_string, parse_json, run_command, string, strings,
    unavailable_search,
};

mod checkout;
mod pipeline;
mod status;
mod timeline;

pub(super) use checkout::checkout;
pub(super) use pipeline::check_details;
pub(super) use timeline::timeline;

fn run(
    forge: &LocalForge,
    cwd: &Path,
    context: &ForgeContext,
    arguments: &[String],
    timeout: Duration,
) -> Result<String, ForgeRuntimeError> {
    let mut command = Command::new(&forge.gitlab_executable);
    command
        .args(arguments)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GLAB_CHECK_UPDATE", "0")
        .env("GITLAB_HOST", &context.host)
        .env_remove("GLAB_REPO")
        .env_remove("GITLAB_REPO");
    if arguments.first().is_some_and(|argument| argument == "api") {
        command.args(["--hostname", &context.host]);
    } else {
        command.args([
            "--repo",
            &format!("https://{}/{}", context.host, context.project_path),
        ]);
    }
    run_command(command, timeout, CommandFamily::Forge)
        .map(|output| output.stdout)
        .map_err(|mut error| {
            if error.kind == ForgeFailureKind::CliMissing {
                "GitLab CLI (glab) is not installed or not in PATH".clone_into(&mut error.message);
            }
            error.forge = Some("gitlab".to_owned());
            error
        })
}

fn read(
    forge: &LocalForge,
    cwd: &Path,
    context: &ForgeContext,
    arguments: &[String],
) -> Result<Value, ForgeRuntimeError> {
    parse_json(&run(forge, cwd, context, arguments, READ_TIMEOUT)?)
}

fn array(value: &Value) -> Result<&[Value], ForgeRuntimeError> {
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| malformed("GitLab response must be an array"))
}

fn project_path<'a>(value: &'a Value, context: &'a ForgeContext) -> &'a str {
    value
        .pointer("/references/full")
        .and_then(Value::as_str)
        .and_then(|reference| reference.split(['!', '#']).next())
        .filter(|path| !path.is_empty())
        .unwrap_or(&context.project_path)
}

fn encode_segment(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            result.push(char::from(byte));
        } else {
            use std::fmt::Write;
            write!(result, "%{byte:02X}").expect("writing to a String cannot fail");
        }
    }
    result
}

fn mr_endpoint(mr: &Value, context: &ForgeContext) -> Result<String, ForgeRuntimeError> {
    Ok(format!(
        "projects/{}/merge_requests/{}",
        encode_segment(project_path(mr, context)),
        integer(mr, "iid")?
    ))
}

fn view(
    forge: &LocalForge,
    cwd: &Path,
    context: &ForgeContext,
    number: u64,
) -> Result<Value, ForgeRuntimeError> {
    read(
        forge,
        cwd,
        context,
        &strings(&["mr", "view", &number.to_string(), "-F", "json"]),
    )
}

/// Resolve a branch's MR; an old closed MR only matches the current HEAD SHA.
/// Uses `forge` in `cwd` with the resolved `context`, retaining GitLab setup states.
/// # Errors
/// Returns local Git, malformed-response or non-authentication CLI failures.
pub(super) fn status(
    forge: &LocalForge,
    cwd: &Path,
    context: &ForgeContext,
) -> Result<PullRequestStatusRead, ForgeRuntimeError> {
    match current(forge, cwd, context) {
        Ok(status) => Ok(PullRequestStatusRead {
            status,
            auth_state: ForgeAuthState::Authenticated,
            forge: Some("gitlab".to_owned()),
        }),
        Err(error)
            if matches!(
                error.kind,
                ForgeFailureKind::CliMissing | ForgeFailureKind::Unauthenticated
            ) =>
        {
            Ok(PullRequestStatusRead {
                status: None,
                auth_state: if error.kind == ForgeFailureKind::CliMissing {
                    ForgeAuthState::CliMissing
                } else {
                    ForgeAuthState::Unauthenticated
                },
                forge: Some("gitlab".to_owned()),
            })
        }
        Err(error) => Err(error),
    }
}

fn current(
    forge: &LocalForge,
    cwd: &Path,
    context: &ForgeContext,
) -> Result<Option<super::PullRequestStatus>, ForgeRuntimeError> {
    let Some(head) = git_optional(
        cwd,
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
        READ_TIMEOUT,
    )?
    else {
        return Ok(None);
    };
    let sha = git_optional(cwd, &["rev-parse", "HEAD"], READ_TIMEOUT)?;
    let value = read(
        forge,
        cwd,
        context,
        &strings(&[
            "mr",
            "list",
            "--all",
            "--source-branch",
            &head,
            "--order",
            "updated_at",
            "--sort",
            "desc",
            "--per-page",
            "100",
            "-F",
            "json",
        ]),
    )?;
    let candidates = array(&value)?;
    let matches_branch =
        |mr: &&Value| mr.get("source_branch").and_then(Value::as_str) == Some(head.as_str());
    let candidate = candidates
        .iter()
        .filter(matches_branch)
        .find(|mr| mr.get("state").and_then(Value::as_str) == Some("opened"))
        .or_else(|| {
            candidates.iter().filter(matches_branch).find(|mr| {
                sha.as_deref()
                    .is_some_and(|sha| mr.get("sha").and_then(Value::as_str) == Some(sha))
            })
        });
    let Some(candidate) = candidate else {
        return Ok(None);
    };
    let mr = view(forge, cwd, context, integer(candidate, "iid")?)?;
    let approvals = read(
        forge,
        cwd,
        context,
        &strings(&["api", &format!("{}/approvals", mr_endpoint(&mr, context)?)]),
    )
    .ok();
    let pipeline = if mr
        .pointer("/head_pipeline/id")
        .and_then(Value::as_u64)
        .is_some()
    {
        match pipeline::read_pipeline(forge, cwd, context, Some(integer(&mr, "iid")?), None) {
            Ok(pipeline) => Some(pipeline),
            Err(error)
                if matches!(
                    error.kind,
                    ForgeFailureKind::CliMissing | ForgeFailureKind::Unauthenticated
                ) =>
            {
                return Err(error);
            }
            Err(_) => None, // Optional jobs must not make the MR disappear.
        }
    } else {
        None
    };
    status::parse(&mr, context, approvals.as_ref(), pipeline.as_ref()).map(Some)
}

/// Search issues and MRs using glab's distinct JSON flags.
/// `input` carries the query, result limit and requested kinds for `cwd`/`context`.
/// Returns sorted neutral results or a CLI/authentication setup state.
/// # Errors
/// Returns non-authentication command failures or malformed search responses.
pub(super) fn search(
    forge: &LocalForge,
    cwd: &Path,
    context: &ForgeContext,
    input: (&str, usize, &[ForgeSearchKind]),
) -> Result<ForgeSearch, ForgeRuntimeError> {
    let (query, limit, kinds) = input;
    let limit = limit.clamp(1, 50);
    let mut items = Vec::new();
    for kind in kinds {
        let (command, output_flag) = match kind {
            ForgeSearchKind::Issue => ("issue", "-O"),
            ForgeSearchKind::ChangeRequest => ("mr", "-F"),
        };
        let mut arguments = strings(&[
            command,
            "list",
            output_flag,
            "json",
            "-P",
            &limit.to_string(),
        ]);
        if !query.trim().is_empty() {
            arguments.extend(strings(&["--search", query]));
        }
        let value = match read(forge, cwd, context, &arguments) {
            Ok(value) => value,
            Err(error) if error.kind == ForgeFailureKind::CliMissing => {
                return Ok(unavailable_search(ForgeAuthState::CliMissing));
            }
            Err(error) if error.kind == ForgeFailureKind::Unauthenticated => {
                return Ok(unavailable_search(ForgeAuthState::Unauthenticated));
            }
            Err(error) => return Err(error),
        };
        for item in array(&value)? {
            items.push(search_item(item, *kind, context)?);
        }
    }
    items.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    items.truncate(limit);
    Ok(ForgeSearch {
        items,
        auth_state: ForgeAuthState::Authenticated,
    })
}

fn search_item(
    value: &Value,
    kind: ForgeSearchKind,
    context: &ForgeContext,
) -> Result<ForgeSearchItem, ForgeRuntimeError> {
    let state = string(value, "state")?;
    Ok(ForgeSearchItem {
        kind,
        forge: Some("gitlab".to_owned()),
        number: integer(value, "iid")?,
        title: string(value, "title")?,
        url: string(value, "web_url")?,
        state: if kind == ForgeSearchKind::ChangeRequest && state == "opened" {
            "open".to_owned()
        } else {
            state
        },
        body: optional_string(value, "description"),
        labels: value
            .get("labels")
            .and_then(Value::as_array)
            .map(|labels| {
                labels
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        project_path: Some(project_path(value, context).to_owned()),
        base_ref_name: optional_string(value, "target_branch"),
        head_ref_name: optional_string(value, "source_branch"),
        updated_at: optional_string(value, "updated_at"),
    })
}

/// Validated creation metadata after the caller pushes the branch.
#[derive(Debug, Clone, Copy)]
pub(super) struct CreateRequest<'a> {
    /// Merge request title.
    pub title: &'a str,
    /// Merge request description.
    pub body: &'a str,
    /// Pushed source branch.
    pub head: &'a str,
    /// Target branch.
    pub base: &'a str,
}

/// Create an MR with glab, preserving the full subgroup path.
/// Uses the already-pushed branch and metadata in `request` for `cwd`/`context`.
/// # Errors
/// Returns command failures or an unparseable creation URL.
pub(super) fn create(
    forge: &LocalForge,
    cwd: &Path,
    context: &ForgeContext,
    request: CreateRequest<'_>,
) -> Result<PullRequestCreated, ForgeRuntimeError> {
    let output = run(
        forge,
        cwd,
        context,
        &strings(&[
            "mr",
            "create",
            "--title",
            request.title,
            "--description",
            request.body,
            "--source-branch",
            request.head,
            "--target-branch",
            request.base,
            "--yes",
        ]),
        WRITE_TIMEOUT,
    )?;
    output
        .split_whitespace()
        .find_map(|word| {
            let (prefix, number) = word.rsplit_once("/-/merge_requests/")?;
            if !prefix.starts_with("https://") && !prefix.starts_with("http://") {
                return None;
            }
            Some(PullRequestCreated {
                url: word.to_owned(),
                number: number.parse().ok()?,
            })
        })
        .ok_or_else(|| {
            malformed("GitLab merge request was created but glab did not return its URL")
        })
}

/// Merge immediately only when GitLab's current MR facts allow it.
/// Validates the fresh status in `read` before applying `method` in `cwd`/`context`.
/// # Errors
/// Rejects unavailable, closed, draft, blocked or scheduled MRs and CLI failures.
pub(super) fn merge(
    forge: &LocalForge,
    cwd: &Path,
    context: &ForgeContext,
    read: &PullRequestStatusRead,
    method: PullRequestMergeMethod,
) -> Result<(), ForgeRuntimeError> {
    let number = current_number(read, "merge")?;
    let mr = read
        .status
        .as_ref()
        .ok_or_else(|| malformed("GitLab MR is unavailable"))?;
    let facts = mr
        .forge_specific
        .as_ref()
        .ok_or_else(|| malformed("GitLab merge facts are unavailable"))?;
    if mr.is_draft
        || mr.state != "open"
        || mr.mergeable != super::PullRequestMergeable::Mergeable
        || facts
            .get("mergeWhenPipelineSucceeds")
            .and_then(Value::as_bool)
            == Some(true)
    {
        return Err(forge_error(
            ForgeFailureKind::Invalid,
            "GitLab does not report this merge request as ready for direct merge",
        ));
    }
    merge_command(forge, cwd, context, number, (method, false))
}

/// Schedule auto-merge for active pipelines or cancel it without merging.
/// `options` selects enable/disable and merge method; `read` supplies fresh MR facts.
/// # Errors
/// Rejects missing identities/methods, unsafe scheduling states or CLI failures.
pub(super) fn auto_merge(
    forge: &LocalForge,
    cwd: &Path,
    context: &ForgeContext,
    read: &PullRequestStatusRead,
    options: (bool, Option<PullRequestMergeMethod>),
) -> Result<(), ForgeRuntimeError> {
    let (enabled, method) = options;
    let number = current_number(read, "auto-merge")?;
    if !enabled {
        run(
            forge,
            cwd,
            context,
            &strings(&[
                "api",
                "--method",
                "POST",
                &format!(
                    "projects/{}/merge_requests/{number}/cancel_merge_when_pipeline_succeeds",
                    encode_segment(&context.project_path)
                ),
            ]),
            WRITE_TIMEOUT,
        )?;
        return Ok(());
    }
    let facts = read
        .status
        .as_ref()
        .and_then(|mr| mr.forge_specific.as_ref())
        .ok_or_else(|| malformed("GitLab auto-merge facts are unavailable"))?;
    if facts
        .get("mergeWhenPipelineSucceeds")
        .and_then(Value::as_bool)
        == Some(true)
        || !facts
            .get("pipelineStatus")
            .and_then(Value::as_str)
            .is_some_and(pipeline::is_active)
    {
        return Err(forge_error(
            ForgeFailureKind::Invalid,
            "GitLab auto-merge requires an active pipeline and must not already be enabled",
        ));
    }
    let method =
        method.ok_or_else(|| forge_error(ForgeFailureKind::Invalid, "mergeMethod is required"))?;
    merge_command(forge, cwd, context, number, (method, true))
}

fn merge_command(
    forge: &LocalForge,
    cwd: &Path,
    context: &ForgeContext,
    number: u64,
    options: (PullRequestMergeMethod, bool),
) -> Result<(), ForgeRuntimeError> {
    let (method, automatic) = options;
    let mut arguments = strings(&[
        "mr",
        "merge",
        &number.to_string(),
        if automatic {
            "--auto-merge"
        } else {
            "--auto-merge=false"
        },
        "--yes",
    ]);
    match method {
        PullRequestMergeMethod::Merge => {}
        PullRequestMergeMethod::Squash => arguments.push("--squash".to_owned()),
        PullRequestMergeMethod::Rebase => arguments.push("--rebase".to_owned()),
    }
    run(forge, cwd, context, &arguments, WRITE_TIMEOUT)?;
    Ok(())
}

#[cfg(test)]
mod tests;
