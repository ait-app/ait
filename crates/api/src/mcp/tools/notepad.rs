//! Workspace notepad tools: placement, authorship and text paging over the notepad RPCs
//! (ADR-296).

use bot::mcp::CallerKind;
use model::ErrorCode;
use serde_json::{Value, json};

use crate::Shared;
use crate::connection::rpc;
use crate::mcp::Scope;
use crate::mcp::catalog::Tool;

use super::take;

/// Characters of a note returned by one `notepad_read` without a `limit`.
const DEFAULT_PAGE: u64 = 4000;
/// Most characters of an Agent's title kept as a note's author title.
const AUTHOR_TITLE_CHARS: usize = 128;
/// Result text for a key no note has.
const NO_NOTE: &str = "No note has this key; call notepad_list for the current keys";

/// Execute notepad `tool` in `state` with `arguments`, placing it in the `caller` Agent's
/// Workspace unless one is named and signing writes as the caller `scope` and `caller` identify.
/// # Errors
/// Returns `InvalidMessage` when no Workspace is named or inherited, and notepad RPC failures.
pub(super) async fn execute(
    state: &Shared,
    tool: &Tool,
    mut arguments: Value,
    scope: Option<&Scope>,
    caller: Option<&Value>,
) -> Result<Value, ErrorCode> {
    if arguments.get("workspaceId").is_none() {
        arguments["workspaceId"] = caller
            .map(|caller| &caller["workspaceId"])
            .filter(|workspace| workspace.is_string())
            .cloned()
            .ok_or(ErrorCode::InvalidMessage)?;
    }
    match tool.name.as_str() {
        "notepad_read" => read(state, &tool.methods[0], arguments).await,
        "notepad_write" => {
            arguments["author"] = author(scope, caller);
            rpc::execute(state, &tool.methods[0], arguments).await
        }
        _ => rpc::execute(state, &tool.methods[0], arguments).await,
    }
}

/// Who writes: the Bot session or Agent a scoped credential belongs to, the Agent a host caller
/// names with `X-Ait-Agent-Id` (claimed, not verified), or an external caller.
fn author(scope: Option<&Scope>, caller: Option<&Value>) -> Value {
    if scope.is_some_and(|scope| matches!(scope.caller.kind, CallerKind::Bot { .. })) {
        return json!({"kind":"bot"});
    }
    let Some(agent) = caller.filter(|agent| agent["id"].is_string()) else {
        return json!({"kind":"external"});
    };
    let mut author = json!({"kind":"agent","agentId":agent["id"]});
    let title = agent["title"].as_str().map(|title| {
        title
            .chars()
            .filter(|character| !character.is_control())
            .take(AUTHOR_TITLE_CHARS)
            .collect::<String>()
    });
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        author["title"] = json!(title);
    }
    author
}

/// Return one page of a note's text with its metadata, or a guiding error for an absent note.
async fn read(state: &Shared, method: &str, mut arguments: Value) -> Result<Value, ErrorCode> {
    let offset = take(&mut arguments, "offset")?
        .as_ref()
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let limit = take(&mut arguments, "limit")?
        .as_ref()
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_PAGE);
    let mut value = rpc::execute(state, method, arguments).await?;
    let mut note = value["note"].take();
    let Some(text) = note["text"].as_str() else {
        value["error"] = json!(NO_NOTE);
        return Ok(value);
    };
    let chars = text.chars().count() as u64;
    let end = offset.saturating_add(limit);
    let page: String = text
        .chars()
        .skip(usize::try_from(offset).unwrap_or(usize::MAX))
        .take(usize::try_from(limit).unwrap_or(usize::MAX))
        .collect();
    note["text"] = json!(page);
    note["chars"] = json!(chars);
    note["nextOffset"] = if end < chars { json!(end) } else { Value::Null };
    note["workspaceId"] = value["workspaceId"].take();
    Ok(note)
}

#[cfg(test)]
mod tests;
