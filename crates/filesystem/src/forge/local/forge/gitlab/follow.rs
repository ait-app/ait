//! One read of a merge request by number for Bot tickets that follow it (ADR-287), in as few
//! glab calls as the data allows: `mr view`, `ci get` when it has a pipeline, its newest notes,
//! the signed-in user, and the project membership of the notes' authors. A failure of any of
//! them fails the whole read, so a partial read never looks like an empty one.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;

use chrono::{DateTime, Utc};
use domain::forge::{AuthorAccess, CHECK_LIMIT, PullItem, PullItemKind, PullSnapshot, PullState};
use serde_json::Value;

use super::{
    ForgeContext, ForgeFailureKind, ForgeRuntimeError, LocalForge, array, encode_segment, integer,
    mr_endpoint, optional_string, pipeline, project_path, read, status, string, strings, view,
};
use crate::forge::local::forge::follow::check;

/// Newest notes read per merge request; the notes API returns them newest first when asked
/// (`sort=desc`, `order_by=created_at`), unlike discussions, whose order is undocumented.
const NOTES_PAGE: usize = 100;
/// GitLab's Developer access level, the lowest that can push to the repository.
const DEVELOPER: u64 = 30;

/// Read GitLab merge request `number` of `context`'s project through `forge` in `cwd`.
///
/// # Errors
/// Returns `NotFound` when the project has no such merge request, and CLI, authentication or
/// malformed-response failures of any of the calls.
pub(in super::super) fn pull_request(
    forge: &LocalForge,
    cwd: &Path,
    context: &ForgeContext,
    number: u64,
) -> Result<PullSnapshot, ForgeRuntimeError> {
    let mr = view(forge, cwd, context, number).map_err(|mut error| {
        if error.message.contains("404") {
            error.kind = ForgeFailureKind::NotFound;
        }
        error
    })?;
    let pipeline = if mr
        .pointer("/head_pipeline/id")
        .and_then(Value::as_u64)
        .is_some()
    {
        Some(pipeline::read_pipeline(
            forge,
            cwd,
            context,
            Some(integer(&mr, "iid")?),
            None,
        )?)
    } else {
        None
    };
    let status = status::parse(&mr, context, None, pipeline.as_ref())?;
    let endpoint = mr_endpoint(&mr, context)?;
    let notes = read(
        forge,
        cwd,
        context,
        &strings(&[
            "api",
            &format!("{endpoint}/notes?sort=desc&order_by=created_at&per_page={NOTES_PAGE}"),
        ]),
    )?;
    let notes: Vec<&Value> = array(&notes)?
        .iter()
        .filter(|note| note.get("system").and_then(Value::as_bool) != Some(true))
        .collect();
    let viewer = string(
        &read(forge, cwd, context, &strings(&["api", "user"]))?,
        "username",
    )?;
    let writers = writers(forge, cwd, context, &mr, &notes)?;
    let web_url = string(&mr, "web_url")?;
    Ok(PullSnapshot {
        number: status.number.unwrap_or(number),
        url: status.url,
        title: status.title,
        state: if status.is_merged {
            PullState::Merged
        } else if matches!(status.state.as_str(), "open" | "locked") {
            // `locked` is GitLab's short transitional state while a merge runs.
            PullState::Open
        } else {
            PullState::Closed
        },
        head: status.head_sha,
        checks: status
            .checks
            .into_iter()
            .take(CHECK_LIMIT)
            .map(check)
            .collect(),
        items: notes
            .into_iter()
            .filter_map(|note| note_item(note, &web_url, &viewer, &writers))
            .collect(),
    })
}

/// The ids of the `notes`' authors who are members of `mr`'s project with Developer access or
/// above, read in one call filtered to those authors.
///
/// # Errors
/// Returns CLI or malformed-response failures.
fn writers(
    forge: &LocalForge,
    cwd: &Path,
    context: &ForgeContext,
    mr: &Value,
    notes: &[&Value],
) -> Result<BTreeSet<u64>, ForgeRuntimeError> {
    let authors: BTreeSet<u64> = notes
        .iter()
        .filter_map(|note| note.pointer("/author/id").and_then(Value::as_u64))
        .collect();
    if authors.is_empty() {
        return Ok(BTreeSet::new());
    }
    let mut endpoint = format!(
        "projects/{}/members/all?per_page={NOTES_PAGE}",
        encode_segment(project_path(mr, context))
    );
    for author in &authors {
        let _ = write!(endpoint, "&user_ids[]={author}");
    }
    let members = read(forge, cwd, context, &strings(&["api", &endpoint]))?;
    Ok(array(&members)?
        .iter()
        .filter(|member| {
            member
                .get("access_level")
                .and_then(Value::as_u64)
                .is_some_and(|level| level >= DEVELOPER)
        })
        .filter_map(|member| member.get("id").and_then(Value::as_u64))
        .collect())
}

/// The neutral entry of one REST `note` of the merge request at `web_url`, written by `viewer`
/// when its author is the signed-in user; `None` without an id or a date.
fn note_item(
    note: &Value,
    web_url: &str,
    viewer: &str,
    writers: &BTreeSet<u64>,
) -> Option<PullItem> {
    let id = note.get("id").and_then(Value::as_u64)?;
    let author = note
        .pointer("/author/username")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let access = if automation(author) {
        AuthorAccess::Bot
    } else if note
        .pointer("/author/id")
        .and_then(Value::as_u64)
        .is_some_and(|author| writers.contains(&author))
    {
        AuthorAccess::Writer
    } else {
        AuthorAccess::Other
    };
    let position = note.get("position").filter(|position| !position.is_null());
    let path = position.and_then(|position| {
        optional_string(position, "new_path").or_else(|| optional_string(position, "old_path"))
    });
    let line = position.and_then(|position| {
        position
            .get("new_line")
            .and_then(Value::as_u64)
            .or_else(|| position.get("old_line").and_then(Value::as_u64))
    });
    Some(PullItem {
        id: id.to_string(),
        kind: if path.is_some() {
            PullItemKind::ReviewComment
        } else {
            PullItemKind::Comment
        },
        own: author.eq_ignore_ascii_case(viewer),
        author: author.to_owned(),
        access,
        body: optional_string(note, "body").unwrap_or_default(),
        url: format!("{web_url}#note_{id}"),
        created_at: optional_string(note, "created_at")
            .and_then(|date| DateTime::parse_from_rfc3339(&date).ok())?
            .with_timezone(&Utc),
        path,
        line,
    })
}

/// Whether GitLab `username` is a bot account: GitLab names the bot users of project and
/// group access tokens `project_<id>_bot_<suffix>` and `group_<id>_bot_<suffix>` and service
/// accounts `service_account_...`, and its own bots end in `-bot`. Note authors carry no bot
/// flag, so this is a naming rule.
fn automation(username: &str) -> bool {
    let name = username.to_ascii_lowercase();
    let token_bot = ["project_", "group_"].iter().any(|prefix| {
        name.strip_prefix(prefix).is_some_and(|rest| {
            rest.split_once("_bot").is_some_and(|(id, _)| {
                !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit())
            })
        })
    });
    token_bot
        || name.starts_with("service_account")
        || name.ends_with("-bot")
        || name.ends_with("[bot]")
}
