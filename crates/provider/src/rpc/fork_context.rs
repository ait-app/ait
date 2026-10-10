//! Text attachment curation from a validated inclusive display-history boundary.

use model::ErrorCode;
use serde_json::{Value, json};

use crate::protocol::native_sessions::ForkRequest;
use crate::storage::timeline::Row;

use crate::timeline_projection::{Entry, project};

mod tools;

pub(crate) fn export(
    request: &ForkRequest,
    epoch: &str,
    rows: &[Row],
    agent: &Value,
) -> Result<Value, ErrorCode> {
    let message = request
        .boundary_message_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty());
    let mut selected = project(rows);
    let boundary = if let Some(cursor) = &request.boundary_cursor {
        if cursor.epoch != epoch {
            return Err(ErrorCode::InvalidMessage);
        }
        Some(
            selected
                .iter()
                .find(|row| row.seq_end == cursor.seq)
                .ok_or(ErrorCode::InvalidMessage)?
                .seq_end,
        )
    } else if let Some(message) = message {
        Some(
            selected
                .iter()
                .rfind(|row| {
                    row.item["type"] == "assistant_message" && row.item["messageId"] == message
                })
                .ok_or(ErrorCode::InvalidMessage)?
                .seq_end,
        )
    } else {
        None
    };
    if let Some(boundary) = boundary {
        if selected
            .iter()
            .any(|entry| entry.seq_start <= boundary && entry.seq_end > boundary)
        {
            return Err(ErrorCode::InvalidMessage);
        }
        selected.retain(|entry| entry.seq_end <= boundary);
    }
    let mut text =
        String::from("<chat-history-summary>\nChat history from a previous Paseo agent.\n");
    for (field, label) in [("title", "Source agent"), ("cwd", "Source directory")] {
        if let Some(value) = agent[field]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            append(&mut text, &format!("{label}: {value}\n"))?;
        }
    }
    append(&mut text, "\n")?;
    append_body(&mut text, &selected)?;
    append(&mut text, "</chat-history-summary>")?;
    super::timeline::bounded(json!({"agentId":request.agent_id,"attachment":{
        "type":"text","mimeType":"text/plain","contextKind":"chat_history","title":"Chat history","text":text},
        "itemCount":selected.len(),"boundaryCursor":request.boundary_cursor,"boundaryMessageId":message,"error":null}))
}

fn append_body(text: &mut String, selected: &[Entry]) -> Result<(), ErrorCode> {
    let mut message = String::new();
    let mut body = false;
    for row in selected {
        let item = &row.item;
        match item["type"].as_str() {
            Some("user_message") => {
                flush_message(text, &mut message)?;
                append(text, "[User] ")?;
                append(text, item["text"].as_str().unwrap_or("").trim())?;
                append(text, "\n")?;
                body = true;
            }
            Some("assistant_message") => {
                let incoming = item["text"].as_str().unwrap_or("").trim();
                if !incoming.is_empty() {
                    if !message.is_empty() {
                        append(&mut message, "\n")?;
                    }
                    append(&mut message, incoming)?;
                }
            }
            Some("tool_call") => {
                flush_message(text, &mut message)?;
                append(text, &tools::summary(item))?;
                append(text, "\n")?;
                if item["detail"]["type"] == "sub_agent" {
                    let log = item["detail"]["log"].as_str().unwrap_or("").trim();
                    if !log.is_empty() {
                        append(text, log)?;
                        append(text, "\n")?;
                    }
                }
                body = true;
            }
            _ => {}
        }
    }
    body |= flush_message(text, &mut message)?;
    if !body {
        append(text, "No chat history to display.\n")?;
    }
    Ok(())
}

fn flush_message(text: &mut String, message: &mut String) -> Result<bool, ErrorCode> {
    let appended = append_message(text, "[Assistant] ", message)?;
    message.clear();
    Ok(appended)
}

fn append_message(output: &mut String, label: &str, text: &str) -> Result<bool, ErrorCode> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(false);
    }
    append(output, label)?;
    append(output, text)?;
    append(output, "\n")?;
    Ok(true)
}

fn append(output: &mut String, text: &str) -> Result<(), ErrorCode> {
    if output.len().saturating_add(text.len()) > 512 * 1024 {
        return Err(ErrorCode::ResourceExhausted);
    }
    output.push_str(text);
    Ok(())
}

#[cfg(test)]
mod tests;
