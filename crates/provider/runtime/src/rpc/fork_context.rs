//! Text attachment curation from a validated inclusive display-history boundary.

use std::collections::VecDeque;

use model::ErrorCode;
use serde_json::{Value, json};

use crate::protocol::native_sessions::ForkRequest;
use crate::storage::timeline::Row;

use crate::timeline_projection::{Entry, project};

mod tools;

const MAX_TEXT_BYTES: usize = 512 * 1024;
const OMITTED_MARKER: &str = "[Earlier conversation omitted to fit the transfer budget.]\n";
const CLOSING_TAG: &str = "</chat-history-summary>";
const MAX_RESPONSE_BYTES: usize = 850 * 1024;

struct BoundedText {
    prefix: String,
    chunks: VecDeque<String>,
    body_len: usize,
    omitted: bool,
}

impl BoundedText {
    fn new(prefix: String) -> Self {
        Self {
            prefix,
            chunks: VecDeque::new(),
            body_len: 0,
            omitted: false,
        }
    }

    fn append(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let content_budget = MAX_TEXT_BYTES
            .saturating_sub(self.prefix.len())
            .saturating_sub(CLOSING_TAG.len())
            .saturating_sub(if self.omitted {
                OMITTED_MARKER.len()
            } else {
                0
            });
        if self.body_len.saturating_add(text.len()) <= content_budget {
            self.chunks.push_back(text.to_owned());
            self.body_len += text.len();
            return;
        }

        self.omitted = true;
        let content_budget = MAX_TEXT_BYTES
            .saturating_sub(self.prefix.len())
            .saturating_sub(CLOSING_TAG.len())
            .saturating_sub(OMITTED_MARKER.len());
        if text.len() >= content_budget {
            self.chunks.clear();
            let suffix = utf8_suffix(text, content_budget);
            self.chunks.push_back(suffix.to_owned());
            self.body_len = suffix.len();
            return;
        }

        self.chunks.push_back(text.to_owned());
        self.body_len += text.len();
        while self.body_len > content_budget {
            let Some(first) = self.chunks.pop_front() else {
                break;
            };
            let excess = self.body_len - content_budget;
            if first.len() <= excess {
                self.body_len -= first.len();
                continue;
            }
            let suffix = utf8_suffix(&first, first.len() - excess);
            self.body_len -= first.len() - suffix.len();
            self.chunks.push_front(suffix.to_owned());
            break;
        }
    }

    fn append_message(&mut self, label: &str, message: &str) {
        let content_len = label.len().saturating_add(message.len()).saturating_add(1);
        let current_budget = self.content_budget(self.omitted);
        if self.body_len.saturating_add(content_len) <= current_budget {
            self.append(label);
            self.append(message);
            self.append("\n");
            return;
        }

        self.omitted = true;
        let budget = self
            .content_budget(true)
            .saturating_sub(label.len())
            .saturating_sub(1);
        let suffix = utf8_suffix(message, budget);
        self.chunks.clear();
        self.chunks.push_back(label.to_owned());
        self.chunks.push_back(suffix.to_owned());
        self.chunks.push_back("\n".to_owned());
        self.body_len = label.len() + suffix.len() + 1;
    }

    fn content_budget(&self, omitted: bool) -> usize {
        MAX_TEXT_BYTES
            .saturating_sub(self.prefix.len())
            .saturating_sub(CLOSING_TAG.len())
            .saturating_sub(if omitted { OMITTED_MARKER.len() } else { 0 })
    }

    fn finish(self) -> String {
        let mut output = String::with_capacity(
            self.prefix.len()
                + usize::from(self.omitted) * OMITTED_MARKER.len()
                + self.body_len
                + CLOSING_TAG.len(),
        );
        output.push_str(&self.prefix);
        if self.omitted {
            output.push_str(OMITTED_MARKER);
        }
        for chunk in self.chunks {
            output.push_str(&chunk);
        }
        output.push_str(CLOSING_TAG);
        output
    }
}

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
    let mut prefix =
        String::from("<chat-history-summary>\nChat history from a previous Ait agent.\n");
    for (field, label) in [("title", "Source agent"), ("cwd", "Source directory")] {
        if let Some(value) = agent[field]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let value: String = value.chars().take(1024).collect();
            prefix.push_str(label);
            prefix.push_str(": ");
            prefix.push_str(&value);
            prefix.push('\n');
        }
    }
    prefix.push('\n');
    let mut text = BoundedText::new(prefix);
    append_body(&mut text, &selected);
    let response = json!({"agentId":request.agent_id,"attachment":{
        "type":"text","mimeType":"text/plain","contextKind":"chat_history","title":"Chat history","text":text.finish()},
        "itemCount":selected.len(),"boundaryCursor":request.boundary_cursor,"boundaryMessageId":message,"error":null});
    bounded_response(response)
}

fn append_body(text: &mut BoundedText, selected: &[Entry]) {
    let mut message = String::new();
    let mut body = false;
    for row in selected {
        let item = &row.item;
        match item["type"].as_str() {
            Some("user_message") => {
                flush_message(text, &mut message);
                append_message(text, "[User] ", item["text"].as_str().unwrap_or("").trim());
                body = true;
            }
            Some("assistant_message") => {
                let incoming = item["text"].as_str().unwrap_or("").trim();
                if !incoming.is_empty() {
                    if !message.is_empty() {
                        message.push('\n');
                    }
                    message.push_str(incoming);
                }
            }
            Some("tool_call") => {
                flush_message(text, &mut message);
                text.append(&tools::summary(item));
                text.append("\n");
                if item["detail"]["type"] == "sub_agent" {
                    let log = item["detail"]["log"].as_str().unwrap_or("").trim();
                    if !log.is_empty() {
                        text.append(log);
                        text.append("\n");
                    }
                }
                body = true;
            }
            _ => {}
        }
    }
    body |= flush_message(text, &mut message);
    if !body {
        text.append("No chat history to display.\n");
    }
}

fn flush_message(text: &mut BoundedText, message: &mut String) -> bool {
    let appended = append_message(text, "[Assistant] ", message);
    message.clear();
    appended
}

fn append_message(output: &mut BoundedText, label: &str, text: &str) -> bool {
    let text = text.trim();
    if text.is_empty() {
        return false;
    }
    output.append_message(label, text);
    true
}

fn bounded_response(mut response: Value) -> Result<Value, ErrorCode> {
    loop {
        let encoded_size = serde_json::to_vec(&response)
            .map_err(|_| ErrorCode::ResourceExhausted)?
            .len();
        if encoded_size <= MAX_RESPONSE_BYTES {
            return super::timeline::bounded(response);
        }
        let current = response["attachment"]["text"]
            .as_str()
            .ok_or(ErrorCode::ResourceExhausted)?;
        let (header, body) = current
            .split_once("\n\n")
            .ok_or(ErrorCode::ResourceExhausted)?;
        let body = body.strip_suffix(CLOSING_TAG).unwrap_or(body);
        let kept = utf8_suffix(body, body.len() / 2);
        response["attachment"]["text"] =
            json!(format!("{header}\n\n{OMITTED_MARKER}{kept}{CLOSING_TAG}"));
    }
}

fn utf8_suffix(text: &str, max_bytes: usize) -> &str {
    let mut start = text.len().saturating_sub(max_bytes);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

#[cfg(test)]
mod tests;
