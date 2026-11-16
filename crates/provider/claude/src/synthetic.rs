//! Classify text Claude Code writes as `user` records but no user typed.

use regex::Regex;
use serde_json::{Value, json};
use std::sync::LazyLock;

const TASK_NOTIFICATION_MARKER: &str = "<task-notification>";

static INTERRUPT_PLACEHOLDER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[Request interrupted by user[^\]]*\]$").expect("valid regex"));
static LOCAL_COMMAND_STDOUT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*<local-command-stdout>[\s\S]*</local-command-stdout>\s*$")
        .expect("valid regex")
});

/// A background task finished; Claude Code wakes the model with a `<task-notification>` prompt.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct TaskNotification {
    pub(super) task_id: Option<String>,
    pub(super) tool_use_id: Option<String>,
    status: Option<String>,
    summary: Option<String>,
    output_file: Option<String>,
    raw: String,
}

impl TaskNotification {
    pub(super) fn parse(text: &str) -> Option<Self> {
        // Claude Code writes the notification as the whole record; a prompt that merely quotes
        // the marker is still the user's text.
        if !text.trim_start().starts_with(TASK_NOTIFICATION_MARKER) {
            return None;
        }
        Some(Self {
            task_id: tag(text, "task-id"),
            tool_use_id: tag(text, "tool-use-id").or_else(|| tag(text, "tool_use_id")),
            status: tag(text, "status"),
            summary: tag(text, "summary"),
            output_file: tag(text, "output-file").or_else(|| tag(text, "output_file")),
            raw: text.trim().to_owned(),
        })
    }

    /// Projects the notification as a completed tool row instead of a user bubble.
    pub(super) fn tool_call(&self, call: &str) -> Value {
        let status = self.status.as_deref().map(str::to_ascii_lowercase);
        let (status, error) = match status.as_deref() {
            Some("failed" | "error") => (
                "failed",
                json!({"message": self.summary.as_deref().unwrap_or("Background task failed")}),
            ),
            Some("canceled" | "cancelled" | "stopped" | "killed") => ("canceled", Value::Null),
            _ => ("completed", Value::Null),
        };
        let label = self.summary.clone().unwrap_or_else(|| {
            self.status.as_deref().map_or_else(
                || "Background task notification".to_owned(),
                |status| format!("Background task {}", status.to_ascii_lowercase()),
            )
        });
        let mut metadata = json!({"synthetic":true,"source":"claude_task_notification"});
        for (key, value) in [
            ("taskId", &self.task_id),
            ("toolUseId", &self.tool_use_id),
            ("status", &self.status),
            ("outputFile", &self.output_file),
        ] {
            if let Some(value) = value {
                metadata[key] = json!(value);
            }
        }
        json!({
            "type":"tool_call",
            "callId":call,
            "name":"task_notification",
            "status":status,
            "error":error,
            "detail":{"type":"plain_text","label":label,"icon":"wrench","text":self.raw},
            "metadata":metadata,
        })
    }
}

/// Claude Code marks `user` records it writes for the model, such as skill bodies and subagent
/// hand-backs, as meta or synthetic.
pub(super) fn injected(record: &Value) -> bool {
    record["isMeta"] == true || record["isSynthetic"] == true
}

/// Returns the text a user bubble should show, or `None` for transcript noise.
///
/// Slash commands are recorded as `<command-name>`/`<command-args>` XML; they read back as the
/// command the user typed.
pub(super) fn user_text(original: &str) -> Option<String> {
    let text = original.trim();
    if text.is_empty()
        || INTERRUPT_PLACEHOLDER.is_match(text)
        || text == "No response requested."
        || LOCAL_COMMAND_STDOUT.is_match(text)
    {
        return None;
    }
    if !text.contains("<command-message>") && !text.contains("<command-name>") {
        return Some(original.to_owned());
    }
    // Prompts that mention the tags without a complete command record stay verbatim.
    let Some(command) = tag(text, "command-name").or_else(|| tag(text, "command-message")) else {
        return Some(original.to_owned());
    };
    let command = if command.starts_with('/') {
        command
    } else {
        format!("/{command}")
    };
    Some(match tag(text, "command-args") {
        Some(args) => format!("{command} {args}"),
        None => command,
    })
}

fn tag(text: &str, name: &str) -> Option<String> {
    let start = text.find(&format!("<{name}>"))? + name.len() + 2;
    let end = start + text[start..].find(&format!("</{name}>"))?;
    Some(text[start..end].trim())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests;
