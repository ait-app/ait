//! GitLab discussions projected into the existing comment/thread protocol.

use std::path::Path;

use serde_json::Value;

use super::{
    ForgeAuthState, ForgeContext, ForgeFailureKind, ForgeRuntimeError, LocalForge, array, integer,
    mr_endpoint, optional_string, read, strings, view,
};
use crate::forge::local::forge::{parse_time, timeline_key};
use crate::forge::ports::forge::{
    PullRequestTimeline, PullRequestTimelineItem, TimelineCommentLocation, TimelineError,
    TimelineErrorKind,
};

const PAGE_SIZE: usize = 100;

/// Read one bounded page of discussions and preserve inline/general reply chains.
/// Looks up `number` in `cwd`/`context` through `forge`; reports pagination truncation.
/// # Errors
/// Returns malformed-response or CLI failures; forbidden/not-found become inline errors.
pub(in super::super) fn timeline(
    forge: &LocalForge,
    cwd: &Path,
    context: &ForgeContext,
    number: u64,
) -> Result<PullRequestTimeline, ForgeRuntimeError> {
    match load(forge, cwd, context, number) {
        Ok(result) => Ok(result),
        Err(error)
            if matches!(
                error.kind,
                ForgeFailureKind::Forbidden | ForgeFailureKind::NotFound
            ) || error.message.contains("404") =>
        {
            Ok(PullRequestTimeline {
                pr_number: number,
                items: Vec::new(),
                truncated: false,
                error: Some(TimelineError {
                    kind: if error.kind == ForgeFailureKind::Forbidden {
                        TimelineErrorKind::Forbidden
                    } else {
                        TimelineErrorKind::NotFound
                    },
                    message: error.message,
                }),
                auth_state: ForgeAuthState::Authenticated,
            })
        }
        Err(error) => Err(error),
    }
}

fn load(
    forge: &LocalForge,
    cwd: &Path,
    context: &ForgeContext,
    number: u64,
) -> Result<PullRequestTimeline, ForgeRuntimeError> {
    let mr = view(forge, cwd, context, number)?;
    let endpoint = format!("{}/discussions", mr_endpoint(&mr, context)?);
    let value = read(
        forge,
        cwd,
        context,
        &strings(&["api", &format!("{endpoint}?per_page={PAGE_SIZE}")]),
    )?;
    let discussions = array(&value)?;
    let mut items = Vec::new();
    for discussion in discussions {
        let notes = array(discussion.get("notes").unwrap_or(&Value::Null))?;
        for note in notes {
            if note.get("system").and_then(Value::as_bool) != Some(true) {
                items.push(comment(note, discussion, notes.len(), &mr)?);
            }
        }
    }
    items.sort_by(|left, right| timeline_key(left).cmp(&timeline_key(right)));
    let truncated = discussions.len() >= PAGE_SIZE
        && read(
            forge,
            cwd,
            context,
            &strings(&[
                "api",
                &format!("{endpoint}?per_page=1&page={}", PAGE_SIZE + 1),
            ]),
        )
        .and_then(|value| array(&value).map(|items| !items.is_empty()))
        .unwrap_or(true);
    Ok(PullRequestTimeline {
        pr_number: number,
        items,
        truncated,
        error: None,
        auth_state: ForgeAuthState::Authenticated,
    })
}

fn comment(
    note: &Value,
    discussion: &Value,
    count: usize,
    mr: &Value,
) -> Result<PullRequestTimelineItem, ForgeRuntimeError> {
    let id = integer(note, "id")?;
    let thread = optional_string(discussion, "id");
    let is_thread = match discussion.get("individual_note").and_then(Value::as_bool) {
        Some(individual) => !individual,
        None => count > 1,
    };
    let resolved = (note.get("resolvable").and_then(Value::as_bool) == Some(true)).then(|| {
        note.get("resolved")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    });
    let location = note.get("position").and_then(|position| {
        let path = optional_string(position, "new_path")
            .or_else(|| optional_string(position, "old_path"))?;
        let line = position
            .get("new_line")
            .and_then(Value::as_u64)
            .or_else(|| position.get("old_line").and_then(Value::as_u64));
        let start_line = position
            .pointer("/line_range/start/new_line")
            .and_then(Value::as_u64)
            .or_else(|| {
                position
                    .pointer("/line_range/start/old_line")
                    .and_then(Value::as_u64)
            })
            .filter(|start| Some(*start) != line);
        Some(TimelineCommentLocation {
            path,
            line,
            start_line,
            thread_id: thread.clone(),
            is_resolved: resolved,
            is_outdated: None,
        })
    });
    Ok(PullRequestTimelineItem::Comment {
        id: id.to_string(),
        author: note
            .get("author")
            .and_then(|author| {
                optional_string(author, "username").or_else(|| optional_string(author, "name"))
            })
            .unwrap_or_else(|| "unknown".to_owned()),
        author_url: note
            .pointer("/author/web_url")
            .and_then(Value::as_str)
            .map(str::to_owned),
        avatar_url: note
            .pointer("/author/avatar_url")
            .and_then(Value::as_str)
            .map(str::to_owned),
        body: optional_string(note, "body").unwrap_or_default(),
        created_at: note
            .get("created_at")
            .and_then(Value::as_str)
            .and_then(parse_time)
            .unwrap_or(0),
        url: format!("{}#note_{id}", super::string(mr, "web_url")?),
        review_id: None,
        thread_id: is_thread.then_some(thread).flatten(),
        thread_is_resolved: if location.is_none() { resolved } else { None },
        location,
    })
}
