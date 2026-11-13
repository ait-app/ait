//! Envelopes that mark agent input Ait wrote rather than a person typed.
//!
//! Every Ait-generated input is fenced in one `<ait-input kind="...">` tag so the model can tell
//! it apart from user text and the timeline can show it as what it is. [`parse`] also reads the
//! tags earlier releases wrote, because provider transcripts keep them.

use std::borrow::Cow;
use std::fmt::Write as _;

use serde::Serialize;

#[cfg(test)]
mod tests;

const OPEN: &str = "<ait-input";
const CLOSE: &str = "</ait-input>";
const LEGACY_SCHEDULE: (&str, &str) = ("<ait-system>", "</ait-system>");
const LEGACY_INSTRUCTIONS: (&str, &str) = ("<ait-bot-instructions>", "</ait-bot-instructions>");
const LEGACY_WAKE: (&str, &str) = ("<ait-notifications>", "</ait-notifications>");

/// The Ait component that wrote an input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum Origin {
    /// One occurrence of a schedule or heartbeat.
    Schedule {
        /// Schedule that fired.
        schedule_id: String,
        /// Run of that schedule.
        run_id: String,
        /// Schedule name, when the schedule has one.
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    /// Built-in Bot instructions prefixed to a Bot turn.
    BotInstructions,
    /// Notifications that woke the Bot.
    BotWake,
}

/// An Ait-generated input split into its origin and the text a person reads as the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope<'a> {
    /// Component that wrote the input.
    pub origin: Origin,
    /// Schedule prompt, notification lines, or the user text that followed Bot instructions.
    pub text: Cow<'a, str>,
}

/// Fences a schedule occurrence's `prompt`.
///
/// # Arguments
///
/// * `schedule_id` - Schedule that fired
/// * `run_id` - Run of that schedule
/// * `name` - Schedule name, if any
/// * `prompt` - Schedule prompt; every `</` in it is escaped so it cannot close the fence
///
/// # Returns
///
/// The input to send to the target Agent.
#[must_use]
pub fn schedule(schedule_id: &str, run_id: &str, name: Option<&str>, prompt: &str) -> String {
    let mut open = format!(
        "{OPEN} kind=\"schedule\" schedule-id=\"{}\" run-id=\"{}\"",
        escape_attribute(schedule_id),
        escape_attribute(run_id)
    );
    if let Some(name) = name {
        let _ = write!(open, " name=\"{}\"", escape_attribute(name));
    }
    format!("{open}>\n{}\n{CLOSE}", escape_body(prompt))
}

/// Fences Bot wake `notifications`, one line per notification.
///
/// Every `</` in the untrusted notification text is escaped so it cannot close the fence.
#[must_use]
pub fn bot_wake(notifications: &str) -> String {
    format!(
        "{OPEN} kind=\"bot_wake\">\n{}\n{CLOSE}",
        escape_body(notifications)
    )
}

/// Prefixes Bot `instructions` to the user's `text`.
///
/// `instructions` are composed by Ait and must already escape every `</` in untrusted parts, so
/// none can close the fence; they are written verbatim.
#[must_use]
pub fn bot_instructions(instructions: &str, text: &str) -> String {
    format!("{OPEN} kind=\"bot_instructions\">\n{instructions}\n{CLOSE}\n\n{text}")
}

/// Splits an Ait-generated `input` into its origin and display text.
///
/// The fence must start the input; a prompt that merely quotes a tag is the user's text. Schedule
/// and wake fences must also end it, while Bot instructions are followed by the user's text.
///
/// # Returns
///
/// `None` when `input` is not a complete Ait envelope.
#[must_use]
pub fn parse(input: &str) -> Option<Envelope<'_>> {
    let trimmed = input.trim();
    if let Some(rest) = trimmed.strip_prefix(OPEN) {
        return parse_current(rest);
    }
    parse_legacy(trimmed)
}

fn parse_current(rest: &str) -> Option<Envelope<'_>> {
    let (attributes, rest) = rest.split_once('>')?;
    let (body, tail) = rest.split_once(CLOSE)?;
    let body = trim_newline(body);
    let origin = match attribute(attributes, "kind")? {
        "schedule" => Origin::Schedule {
            schedule_id: unescape_attribute(attribute(attributes, "schedule-id")?),
            run_id: unescape_attribute(attribute(attributes, "run-id")?),
            name: attribute(attributes, "name").map(unescape_attribute),
        },
        "bot_wake" => Origin::BotWake,
        "bot_instructions" => {
            return Some(Envelope {
                origin: Origin::BotInstructions,
                text: Cow::Borrowed(tail.trim_start()),
            });
        }
        _ => return None,
    };
    tail.trim().is_empty().then(|| Envelope {
        origin,
        text: unescape_body(body),
    })
}

fn parse_legacy(input: &str) -> Option<Envelope<'_>> {
    if let Some(body) = fenced(input, LEGACY_SCHEDULE) {
        let (heading, prompt) = body.split_once('\n')?;
        let (name, schedule_id, run_id) = legacy_heading(heading)?;
        return Some(Envelope {
            origin: Origin::Schedule {
                schedule_id: schedule_id.to_owned(),
                run_id: run_id.to_owned(),
                name: name.map(str::to_owned),
            },
            text: Cow::Borrowed(prompt),
        });
    }
    if let Some(body) = fenced(input, LEGACY_WAKE) {
        return Some(Envelope {
            origin: Origin::BotWake,
            text: unescape_body(body),
        });
    }
    let rest = input.strip_prefix(LEGACY_INSTRUCTIONS.0)?;
    let (_, text) = rest.split_once(LEGACY_INSTRUCTIONS.1)?;
    Some(Envelope {
        origin: Origin::BotInstructions,
        text: Cow::Borrowed(text.trim_start()),
    })
}

/// Body of `input` when the `(open, close)` fence spans all of it.
fn fenced<'a>(input: &'a str, (open, close): (&str, &str)) -> Option<&'a str> {
    let body = input.strip_prefix(open)?.strip_suffix(close)?;
    Some(trim_newline(body))
}

/// Reads `Schedule "name" fired (id=..., run=...).` or `Schedule fired (id=..., run=...).`.
fn legacy_heading(heading: &str) -> Option<(Option<&str>, &str, &str)> {
    let rest = heading.strip_prefix("Schedule ")?;
    let (name, ids) = match rest.strip_prefix('"') {
        Some(quoted) => {
            let (name, ids) = quoted.rsplit_once("\" fired (")?;
            (Some(name), ids)
        }
        None => (None, rest.strip_prefix("fired (")?),
    };
    let (schedule_id, run_id) = ids
        .strip_prefix("id=")?
        .strip_suffix(").")?
        .split_once(", run=")?;
    Some((name, schedule_id, run_id))
}

/// Value of `name="..."` in an open tag's attribute list.
fn attribute<'a>(attributes: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = attributes;
    while let Some((key, after)) = rest.split_once("=\"") {
        let (value, next) = after.split_once('"')?;
        if key.trim() == name {
            return Some(value);
        }
        rest = next;
    }
    None
}

fn trim_newline(body: &str) -> &str {
    let body = body.strip_prefix('\n').unwrap_or(body);
    body.strip_suffix('\n').unwrap_or(body)
}

fn escape_body(text: &str) -> Cow<'_, str> {
    if text.contains("</") {
        Cow::Owned(text.replace("</", "<\\/"))
    } else {
        Cow::Borrowed(text)
    }
}

fn unescape_body(text: &str) -> Cow<'_, str> {
    if text.contains("<\\/") {
        Cow::Owned(text.replace("<\\/", "</"))
    } else {
        Cow::Borrowed(text)
    }
}

fn escape_attribute(value: &str) -> Cow<'_, str> {
    if !value.contains(['&', '"', '<', '>']) {
        return Cow::Borrowed(value);
    }
    let mut escaped = String::with_capacity(value.len() + 8);
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '"' => escaped.push_str("&quot;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            other => escaped.push(other),
        }
    }
    Cow::Owned(escaped)
}

fn unescape_attribute(value: &str) -> String {
    if !value.contains('&') {
        return value.to_owned();
    }
    value
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}
