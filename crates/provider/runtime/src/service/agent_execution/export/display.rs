//! Human-readable rendering of the canonical `AgentTimelineItem` contract, not native UI state.
//! Tool titles and fields follow `packages/protocol/src/tool-call-display.ts` and `agent-types.ts`.

use std::io::Write;

use model::ErrorCode;
use serde::Deserialize;
use serde_json::Value;

use crate::ports::timeline_export::{Format, Scope};
use crate::timeline_projection::Entry;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Content<'a> {
    UserMessage {
        #[serde(borrow)]
        text: &'a str,
    },
    AssistantMessage {
        #[serde(borrow)]
        text: &'a str,
    },
    Reasoning {
        #[serde(borrow)]
        text: &'a str,
    },
    ToolCall {
        #[serde(borrow)]
        name: &'a str,
        status: &'a str,
    },
    Todo,
    Error {
        #[serde(borrow)]
        message: &'a str,
    },
    Notification {
        #[serde(borrow)]
        message: &'a str,
        level: &'a str,
    },
    Compaction {
        #[serde(borrow)]
        status: &'a str,
    },
    Plugin {
        #[serde(borrow)]
        kind: &'a str,
        #[serde(rename = "pluginId")]
        plugin_id: &'a str,
    },
    Voice {
        kind: VoiceKind,
        #[serde(borrow)]
        role: Option<&'a str>,
        text: Option<&'a str>,
        status: &'a str,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum VoiceKind {
    Transcript,
    Session,
}

#[derive(Deserialize)]
struct Task<'a> {
    #[serde(borrow)]
    text: &'a str,
    completed: bool,
}

pub(super) fn entry(
    writer: &mut impl Write,
    format: Format,
    scope: Scope,
    entry: &Entry,
) -> Result<bool, ErrorCode> {
    let content = Content::deserialize(&entry.item).map_err(|_| ErrorCode::AgentIo)?;
    let conversation = matches!(
        content,
        Content::UserMessage { .. }
            | Content::AssistantMessage { .. }
            | Content::Voice {
                kind: VoiceKind::Transcript,
                ..
            }
    );
    if scope == Scope::Conversation && !conversation {
        return Ok(false);
    }
    let title = match &content {
        Content::UserMessage { .. } => "User",
        Content::AssistantMessage { .. } => "Assistant",
        Content::Reasoning { .. } => "Reasoning",
        Content::ToolCall { name, .. } => name,
        Content::Todo => "Tasks",
        Content::Error { .. } => "Error",
        Content::Notification { .. } => "Notification",
        Content::Compaction { .. } => "Context compaction",
        Content::Plugin { kind, .. } => kind,
        Content::Voice {
            kind: VoiceKind::Transcript,
            role,
            ..
        } => match role {
            Some("user") => "User voice transcript",
            Some("assistant") => "Assistant voice transcript",
            _ => "Voice transcript",
        },
        Content::Voice {
            kind: VoiceKind::Session,
            ..
        } => "Voice session",
        Content::Unknown => "Unknown saved item",
    };
    heading(writer, format, title).map_err(|error| super::io(&error))?;
    field_text(writer, format, "Time", &entry.timestamp).map_err(|error| super::io(&error))?;
    field_text(writer, format, "Provider", &entry.provider).map_err(|error| super::io(&error))?;
    if let Some(turn) = &entry.turn_id {
        field_text(writer, format, "Turn", turn).map_err(|error| super::io(&error))?;
    }
    let excluded: &[&str] = match content {
        Content::UserMessage { text: body }
        | Content::AssistantMessage { text: body }
        | Content::Reasoning { text: body } => {
            text(writer, format, body).map_err(|error| super::io(&error))?;
            &["type", "text"]
        }
        Content::Error { message } => {
            text(writer, format, message).map_err(|error| super::io(&error))?;
            &["type", "message"]
        }
        Content::Notification { message, level } => {
            field_text(writer, format, "Level", level).map_err(|error| super::io(&error))?;
            text(writer, format, message).map_err(|error| super::io(&error))?;
            &["type", "message", "level"]
        }
        Content::ToolCall { status, .. } | Content::Compaction { status } => {
            field_text(writer, format, "Status", status).map_err(|error| super::io(&error))?;
            &["type", "name", "status"]
        }
        Content::Plugin { plugin_id, .. } => {
            field_text(writer, format, "Plugin", plugin_id).map_err(|error| super::io(&error))?;
            &["type", "kind", "pluginId"]
        }
        Content::Voice {
            kind,
            text: body,
            status,
            ..
        } => {
            field_text(writer, format, "Status", status).map_err(|error| super::io(&error))?;
            if matches!(kind, VoiceKind::Transcript) {
                text(writer, format, body.ok_or(ErrorCode::AgentIo)?)
                    .map_err(|error| super::io(&error))?;
            }
            &["type", "kind", "role", "text", "status"]
        }
        Content::Todo => {
            tasks(writer, format, &entry.item).map_err(|error| super::io(&error))?;
            &["type", "items"]
        }
        Content::Unknown => {
            unknown(writer, format, &entry.item).map_err(|error| super::io(&error))?;
            return Ok(true);
        }
    };
    fields(writer, format, &entry.item, excluded).map_err(|error| super::io(&error))?;
    Ok(true)
}

/// Stream the unknown-kind fallback through the same output bound, without first allocating an
/// arbitrarily expanded pretty-printed JSON string.
fn unknown(writer: &mut impl Write, format: Format, value: &Value) -> std::io::Result<()> {
    if format == Format::Html {
        writeln!(writer, "<pre><code>")?;
    }
    let mut escaped = JsonOutput {
        writer,
        format,
        line_start: true,
    };
    serde_json::to_writer_pretty(&mut escaped, value).map_err(|error| {
        std::io::Error::new(
            error.io_error_kind().unwrap_or(std::io::ErrorKind::Other),
            error,
        )
    })?;
    match format {
        Format::Markdown => writeln!(escaped.writer, "\n"),
        Format::Html => writeln!(escaped.writer, "</code></pre>"),
    }
}

struct JsonOutput<'a, W> {
    writer: &'a mut W,
    format: Format,
    line_start: bool,
}

impl<W: Write> Write for JsonOutput<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        match self.format {
            Format::Html => super::escape_html(self.writer, bytes)?,
            Format::Markdown => {
                for line in bytes.split_inclusive(|byte| *byte == b'\n') {
                    if self.line_start {
                        self.writer.write_all(b"    ")?;
                    }
                    self.writer.write_all(line)?;
                    self.line_start = line.last() == Some(&b'\n');
                }
            }
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

fn heading(writer: &mut impl Write, format: Format, title: &str) -> std::io::Result<()> {
    match format {
        Format::Markdown => {
            write!(writer, "## ")?;
            inline(writer, format, title)?;
            writeln!(writer, "\n")
        }
        Format::Html => {
            write!(writer, "<h2>")?;
            inline(writer, format, title)?;
            writeln!(writer, "</h2>")
        }
    }
}

fn field_text(
    writer: &mut impl Write,
    format: Format,
    label: &str,
    value: &str,
) -> std::io::Result<()> {
    match format {
        Format::Markdown => {
            write!(writer, "**")?;
            inline(writer, format, label)?;
            write!(writer, ":** ")?;
            inline(writer, format, value)?;
            writeln!(writer, "\n")
        }
        Format::Html => {
            write!(writer, "<p><strong>")?;
            inline(writer, format, label)?;
            write!(writer, ":</strong> ")?;
            inline(writer, format, value)?;
            writeln!(writer, "</p>")
        }
    }
}

fn text(writer: &mut impl Write, format: Format, body: &str) -> std::io::Result<()> {
    match format {
        // Preserve the actual conversation Markdown instead of quoting it as a serialized value.
        Format::Markdown => writeln!(writer, "{body}\n"),
        Format::Html => {
            write!(writer, "<p>")?;
            for (index, line) in body.split('\n').enumerate() {
                if index > 0 {
                    writeln!(writer, "<br>")?;
                }
                super::escape_html(writer, line.as_bytes())?;
            }
            writeln!(writer, "</p>")
        }
    }
}

fn tasks(writer: &mut impl Write, format: Format, item: &Value) -> std::io::Result<()> {
    let items = item
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| std::io::Error::other("Invalid saved task list"))?;
    if format == Format::Html {
        writeln!(writer, "<ul>")?;
    }
    for item in items {
        let task = Task::deserialize(item).map_err(std::io::Error::other)?;
        let checked = if task.completed { "x" } else { " " };
        match format {
            Format::Markdown => write!(writer, "- [{checked}] ")?,
            Format::Html => write!(writer, "<li>[{checked}] ")?,
        }
        inline(writer, format, task.text)?;
        writeln!(writer)?;
        fields(writer, format, item, &["text", "completed"])?;
        if format == Format::Html {
            writeln!(writer, "</li>")?;
        }
    }
    if format == Format::Html {
        writeln!(writer, "</ul>")?;
    }
    writeln!(writer)
}

/// Known cards retain all durable fields, including future additions, as titled readable values.
/// This is deliberately not a JSON dump; arbitrary unknown item kinds alone use that fallback.
fn fields(
    writer: &mut impl Write,
    format: Format,
    value: &Value,
    excluded: &[&str],
) -> std::io::Result<()> {
    let Some(object) = value.as_object() else {
        return structured(writer, format, value);
    };
    if format == Format::Html {
        writeln!(writer, "<dl>")?;
    }
    for (key, value) in object {
        if excluded.contains(&key.as_str()) {
            continue;
        }
        let label = label(key);
        match format {
            Format::Markdown => {
                write!(writer, "**")?;
                inline(writer, format, label)?;
                writeln!(writer, ":**\n")?;
            }
            Format::Html => {
                write!(writer, "<dt>")?;
                inline(writer, format, label)?;
                writeln!(writer, "</dt><dd>")?;
            }
        }
        if key == "imageSources" {
            let references = value
                .as_array()
                .ok_or_else(|| std::io::Error::other("Invalid saved image references"))?;
            for reference in references {
                let reference = reference
                    .as_str()
                    .ok_or_else(|| std::io::Error::other("Invalid saved image reference"))?;
                field_text(writer, format, "Reference", reference)?;
            }
        } else if matches!(
            key.as_str(),
            "command" | "output" | "log" | "content" | "oldString" | "newString" | "unifiedDiff"
        ) && let Some(value) = value.as_str()
        {
            code(writer, format, value, "")?;
        } else {
            structured(writer, format, value)?;
        }
        if format == Format::Html {
            writeln!(writer, "</dd>")?;
        }
    }
    if format == Format::Html {
        writeln!(writer, "</dl>")?;
    }
    Ok(())
}

fn structured(writer: &mut impl Write, format: Format, value: &Value) -> std::io::Result<()> {
    match value {
        Value::Object(_) => fields(writer, format, value, &[]),
        Value::Array(values) => {
            if format == Format::Html {
                writeln!(writer, "<ol>")?;
            }
            for (index, value) in values.iter().enumerate() {
                match format {
                    Format::Markdown => writeln!(writer, "{}. ", index + 1)?,
                    Format::Html => writeln!(writer, "<li>")?,
                }
                structured(writer, format, value)?;
                if format == Format::Html {
                    writeln!(writer, "</li>")?;
                }
            }
            if format == Format::Html {
                writeln!(writer, "</ol>")?;
            }
            Ok(())
        }
        Value::String(value) => text(writer, format, value),
        Value::Null => text(writer, format, "Not recorded"),
        Value::Bool(value) => text(writer, format, if *value { "true" } else { "false" }),
        Value::Number(value) => text(writer, format, &value.to_string()),
    }
}

fn code(
    writer: &mut impl Write,
    format: Format,
    value: &str,
    language: &str,
) -> std::io::Result<()> {
    match format {
        Format::Markdown => {
            let longest = value
                .as_bytes()
                .split(|byte| *byte != b'`')
                .map(<[u8]>::len)
                .max()
                .unwrap_or(0);
            let fence = "`".repeat(3.max(longest + 1));
            writeln!(writer, "{fence}{language}\n{value}\n{fence}\n")
        }
        Format::Html => {
            write!(writer, "<pre><code>")?;
            super::escape_html(writer, value.as_bytes())?;
            writeln!(writer, "</code></pre>")
        }
    }
}

fn inline(writer: &mut impl Write, format: Format, value: &str) -> std::io::Result<()> {
    if format == Format::Html {
        return super::escape_html(writer, value.as_bytes());
    }
    for character in value.chars() {
        if character == '\n' || character == '\r' {
            write!(writer, " ")?;
            continue;
        }
        if "\\`*_{{}}[]()#+-.!<>|".contains(character) {
            write!(writer, "\\")?;
        }
        write!(writer, "{character}")?;
    }
    Ok(())
}

fn label(key: &str) -> &str {
    match key {
        "imageSources" => "Image references (not embedded)",
        "voiceTranscript" => "Voice transcript",
        "messageId" => "Message ID",
        "clientMessageId" => "Client message ID",
        "callId" => "Call ID",
        "sessionId" => "Session ID",
        "childSessionId" => "Child session ID",
        "pluginId" => "Plugin",
        "data" => "Saved result",
        "detail" => "Details",
        "metadata" => "Saved metadata",
        "filePath" => "File path",
        "filePaths" => "File paths",
        "cwd" => "Working directory",
        "exitCode" => "Exit code",
        "oldString" => "Previous content",
        "newString" => "New content",
        "unifiedDiff" => "Diff",
        "worktreePath" => "Worktree path",
        "branchName" => "Branch",
        "subAgentType" => "Child agent type",
        "activeForm" => "Current activity",
        "compactionId" => "Compaction ID",
        "preTokens" => "Tokens before compaction",
        "webResults" => "Web results",
        "durationMs" => "Duration (ms)",
        "durationSeconds" => "Duration (seconds)",
        "numFiles" => "File count",
        "numMatches" => "Match count",
        "toolName" => "Tool",
        "codeText" => "Response status",
        "type" => "Type",
        "name" => "Name",
        "status" => "Status",
        "title" => "Title",
        "body" | "content" => "Content",
        "text" => "Text",
        "command" => "Command",
        "output" => "Output",
        "log" => "Log",
        "query" => "Query",
        "url" => "URL",
        "prompt" => "Prompt",
        "result" => "Result",
        "description" => "Description",
        "error" => "Error",
        "summary" => "Summary",
        "commands" => "Commands",
        "actions" => "Actions",
        "input" => "Input",
        "label" => "Label",
        "role" => "Role",
        "version" => "Version",
        "id" => "ID",
        "trigger" => "Trigger",
        "annotations" => "Annotations",
        "truncated" => "Source was truncated",
        _ => key,
    }
}
