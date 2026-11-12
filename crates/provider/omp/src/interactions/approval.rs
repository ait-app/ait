//! OMP tool approvals (D14). A `select` titled `Allow tool: bash|edit|write` becomes a host
//! tool permission with shell, edit or write detail, as Paseo's `rpc-ui-permission-mapper.ts`
//! maps it. Any other `select` stays a generic question.

use std::borrow::Cow;

use serde_json::{Value, json};

use crate::PROVIDER;
use provider_core::ports::agent_session::AgentSessionError;

const TITLE_PREFIX: &str = "Allow tool: ";
const DENY: &str = "Deny";
/// OMP 18.18.8 names the one-time approval `Approve`, or `Approve Once` when it also offers
/// folder or file grants (`always-ask-cached`).
const ONCE: [&str; 2] = ["Approve", "Approve Once"];
/// Prefix of the folder grant option; OMP then appends a `Folder: <label>` listing to the title.
const FOLDER_GRANT: &str = "Approve Directory: ";
/// Bytes of a command or written content shown on the card. OMP 18.18.8 already elides each
/// to 2000 characters; older releases send them whole.
const MAX_DETAIL: usize = 16 * 1024;
/// Appended to detail cut at [`MAX_DETAIL`].
const TRUNCATED: &str = "\n[Truncated; the full text remains in OMP.]";

/// Map a native approval `select` to a host tool permission.
///
/// # Arguments
///
/// * `native` - Native `extension_ui_request` with method `select`.
/// * `id` - Its validated request identity.
///
/// # Returns
///
/// The host request, or `None` when the title names another tool, lacks the tool's detail, or
/// the options are not OMP's approval choices: `Deny`, a one-time approval and optional
/// `Approve …`/`Always Approve …` session or grant scopes.
pub(super) fn request(native: &Value, id: &str) -> Option<Value> {
    let title = native["title"].as_str()?;
    let options = native["options"]
        .as_array()?
        .iter()
        .map(Value::as_str)
        .collect::<Option<Vec<_>>>()?;
    if !options.contains(&DENY)
        || !options.iter().any(|option| ONCE.contains(option))
        || !options
            .iter()
            .all(|option| *option == DENY || is_approval(option))
    {
        return None;
    }
    let (first, body) = title.split_once('\n').unwrap_or((title, ""));
    let tool = first.trim().strip_prefix(TITLE_PREFIX)?.trim();
    let (detail, description) = match tool {
        "bash" => bash(body)?,
        "edit" => edit(body)?,
        "write" => write(body, &options)?,
        _ => return None,
    };
    Some(
        json!({"id":id,"provider":PROVIDER,"name":tool,"kind":"tool",
        "title":format!("{TITLE_PREFIX}{tool}"),"description":description,"detail":detail,
        "actions":actions(&options),"metadata":{"extensionUiMethod":"select","toolName":tool}}),
    )
}

/// Build the native reply choosing the option of the host's selected action.
///
/// # Arguments
///
/// * `id` - Native request identity.
/// * `request` - Host request built by [`request`].
/// * `response` - Host answer with `behavior` and an optional `selectedActionId`.
///
/// # Returns
///
/// `{value: <option>}`; without a selected action, allow picks the one-time approval and deny
/// picks `Deny`, as Paseo answers.
///
/// # Errors
///
/// Returns `AgentSessionError::Rejected` for an unknown behavior, or a selected action that
/// was not offered or whose behavior differs.
pub(super) fn reply(
    id: &str,
    request: &Value,
    response: &Value,
) -> Result<Value, AgentSessionError> {
    let behavior = response["behavior"]
        .as_str()
        .filter(|behavior| matches!(*behavior, "allow" | "deny"))
        .ok_or(AgentSessionError::Rejected)?;
    let wanted = response
        .get("selectedActionId")
        .map_or(
            Some(if behavior == "allow" {
                "approve"
            } else {
                "deny"
            }),
            Value::as_str,
        )
        .ok_or(AgentSessionError::Rejected)?;
    let action = request["actions"]
        .as_array()
        .and_then(|actions| actions.iter().find(|action| action["id"] == wanted))
        .filter(|action| action["behavior"] == behavior)
        .ok_or(AgentSessionError::Rejected)?;
    Ok(json!({"type":"extension_ui_response","id":id,"value":action["label"]}))
}

fn is_approval(option: &str) -> bool {
    option.starts_with("Approve") || option.starts_with("Always Approve")
}

/// Deny first, then the one-time approval, then the remaining scopes in native order.
fn actions(options: &[&str]) -> Vec<Value> {
    let once = options
        .iter()
        .find(|option| ONCE.contains(option))
        .copied()
        .unwrap_or(ONCE[0]);
    let mut actions = Vec::with_capacity(options.len());
    actions.push(
        json!({"id":"deny","label":DENY,"behavior":"deny","variant":"danger",
        "intent":"dismiss"}),
    );
    actions.push(json!({"id":"approve","label":once,"behavior":"allow","variant":"primary"}));
    actions.extend(
        options
            .iter()
            .enumerate()
            .filter(|(_, option)| **option != DENY && **option != once)
            .map(|(index, option)| {
                json!({"id":format!("approve-{index}"),"label":option,"behavior":"allow",
                    "variant":"secondary"})
            }),
    );
    actions
}

/// The command after the first `Command:` line, through the end of the body.
fn bash(body: &str) -> Option<(Value, String)> {
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        if let Some(rest) = line
            .trim_start_matches([' ', '\t'])
            .strip_prefix("Command:")
        {
            let start = offset + line.len() - rest.len();
            let command = &body[start..];
            let command = command.strip_prefix([' ', '\t']).unwrap_or(command);
            if command.is_empty() {
                return None;
            }
            let command = bounded(command);
            return Some((
                json!({"type":"shell","command":command}),
                format!("Command: {command}"),
            ));
        }
        offset += line.len();
    }
    None
}

fn edit(body: &str) -> Option<(Value, String)> {
    let path = body.lines().find_map(|line| prefixed(line, "File:"))?;
    Some((
        json!({"type":"edit","filePath":path}),
        format!("File: {path}"),
    ))
}

/// `Path:`, then the content after a later `Content:` line. A folder grant's `Folder:`
/// listing, which OMP appends after the content, is not part of it.
fn write(body: &str, options: &[&str]) -> Option<(Value, String)> {
    let lines: Vec<&str> = body.lines().collect();
    let (path_index, path) = lines
        .iter()
        .enumerate()
        .find_map(|(index, line)| Some((index, prefixed(line, "Path:")?)))?;
    let content_index = path_index
        + 1
        + lines[path_index + 1..]
            .iter()
            .position(|line| line.trim() == "Content:")?;
    let mut content = &lines[content_index + 1..];
    if let Some(folder) = options
        .iter()
        .find_map(|option| option.strip_prefix(FOLDER_GRANT))
    {
        let listing = format!("Folder: {folder}");
        if let Some(end) = content.iter().rposition(|line| line.starts_with(&listing)) {
            content = &content[..end];
        }
    }
    let content = content.join("\n");
    let content = bounded(&content);
    Some((
        json!({"type":"write","filePath":path,"content":content}),
        format!("Path: {path}"),
    ))
}

/// `text` cut to [`MAX_DETAIL`] bytes at a character boundary, marked when cut.
fn bounded(text: &str) -> Cow<'_, str> {
    if text.len() <= MAX_DETAIL {
        return Cow::Borrowed(text);
    }
    let end = text.floor_char_boundary(MAX_DETAIL);
    Cow::Owned(format!("{}{TRUNCATED}", &text[..end]))
}

/// The non-empty trimmed value after `prefix` on a trimmed `line`.
fn prefixed<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    Some(line.trim().strip_prefix(prefix)?.trim()).filter(|value| !value.is_empty())
}
