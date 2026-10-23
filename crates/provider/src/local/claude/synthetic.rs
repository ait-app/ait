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
        if !text.contains(TASK_NOTIFICATION_MARKER) {
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
    let command = tag(text, "command-name").or_else(|| tag(text, "command-message"))?;
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
mod tests {
    use super::*;

    #[test]
    fn task_notifications_become_tool_rows() {
        let text = "<task-notification>\n<task-id>b7ktc4esc</task-id>\n<tool-use-id>toolu_1</tool-use-id>\n<output-file>/tmp/b7.output</output-file>\n<status>completed</status>\n<summary>Background command \"Wait for CI\" completed (exit code 0)</summary>\n</task-notification>";
        let notification = TaskNotification::parse(text).unwrap();
        assert_eq!(notification.task_id.as_deref(), Some("b7ktc4esc"));
        assert_eq!(notification.tool_use_id.as_deref(), Some("toolu_1"));
        let item = notification.tool_call("task_notification_x");
        assert_eq!(item["status"], "completed");
        assert_eq!(
            item["detail"]["label"],
            "Background command \"Wait for CI\" completed (exit code 0)"
        );
        assert_eq!(item["metadata"]["outputFile"], "/tmp/b7.output");

        let failed = TaskNotification::parse(
            "<task-notification><status>failed</status></task-notification>",
        )
        .unwrap()
        .tool_call("id");
        assert_eq!(failed["status"], "failed");
        assert_eq!(failed["detail"]["label"], "Background task failed");
        assert_eq!(TaskNotification::parse("plain prompt"), None);
    }

    #[test]
    fn transcript_noise_is_hidden_and_commands_read_back() {
        for noise in [
            "[Request interrupted by user]",
            "[Request interrupted by user for tool use]",
            "No response requested.",
            "<local-command-stdout>Set model to opus</local-command-stdout>",
            "  ",
        ] {
            assert_eq!(user_text(noise), None, "{noise}");
        }
        assert_eq!(
            user_text("<command-message>review</command-message>\n<command-name>/review</command-name>\n<command-args>#12</command-args>").as_deref(),
            Some("/review #12")
        );
        assert_eq!(
            user_text("<command-name>clear</command-name>").as_deref(),
            Some("/clear")
        );
        assert_eq!(user_text(" hello ").as_deref(), Some(" hello "));
    }
}
