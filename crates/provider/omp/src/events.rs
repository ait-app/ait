//! Display projection of OMP custom messages (D11), runtime telemetry (M2, M3) and todo
//! lists (M4), ported from Paseo's OMP mappers (`advisor-message.ts`, `system-notice.ts`,
//! `custom-message.ts`, `event-mapper.ts` and `todo-mapper.ts`).
//!
//! Each function maps one native value to a timeline item and assigns no timeline keys; a
//! malformed value maps to none, as Paseo drops it.

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::stream::preview;

const NOTICE_OPEN: &str = "<system-notice>";
const NOTICE_CLOSE: &str = "</system-notice>";
/// Custom message details larger than this stay in OMP instead of the row's metadata.
const MAX_DETAILS_BYTES: usize = 64 * 1024;

/// Project a displayed custom message, other than a skill prompt, whose display text is
/// `text`: an advisor row, a `<system-notice>` notification, or a plain-text row named after
/// its custom type.
///
/// # Arguments
///
/// * `message` - The native custom message.
/// * `text` - Its nonempty display text.
/// * `call_id` - The stable call id of a plain-text row.
pub(super) fn custom(message: &Value, text: &str, call_id: &str) -> Value {
    if message["customType"] == "advisor" {
        return advisor(message, text);
    }
    if let Some(notification) = system_notice(text) {
        return notification;
    }
    let custom_type = message["customType"]
        .as_str()
        .filter(|kind| !kind.trim().is_empty())
        .unwrap_or("custom-message");
    let mut metadata = json!({"synthetic":true,"customType":custom_type});
    let details = &message["details"];
    if !details.is_null() && details.to_string().len() <= MAX_DETAILS_BYTES {
        metadata["details"] = details.clone();
    }
    json!({"type":"tool_call","callId":call_id,"name":custom_type,"status":"completed",
        "detail":{"type":"plain_text","text":text},"metadata":metadata,"error":null})
}

/// An advisor row listing each severity-tagged note, as Paseo's advisor mapper.
fn advisor(message: &Value, text: &str) -> Value {
    let notes: Vec<(String, bool)> = message["details"]["notes"]
        .as_array()
        .map(|notes| notes.iter().filter_map(advisor_note).collect())
        .unwrap_or_default();
    let blockers = notes.iter().filter(|(_, blocker)| *blocker).count();
    let label = match notes.len() {
        0 => "Advisor".to_owned(),
        count => {
            let label = format!("Advisor · {count} {}", plural(count, "note", "notes"));
            if blockers == 0 {
                label
            } else {
                format!(
                    "{label} · {blockers} {}",
                    plural(blockers, "blocker", "blockers")
                )
            }
        }
    };
    let body = if notes.is_empty() {
        text.to_owned()
    } else {
        let lines: Vec<&str> = notes.iter().map(|(line, _)| line.as_str()).collect();
        preview(&lines.join("\n\n"))
    };
    let id = message["id"]
        .as_str()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map_or_else(|| digest(&[text.trim()]), str::to_owned);
    json!({"type":"tool_call","callId":format!("omp-advisor:{id}"),"name":"advisor",
        "status":"completed","detail":{"type":"plain_text","label":label,"text":body,"icon":"brain"},
        "metadata":{"synthetic":true,"source":"omp_advisor","noteCount":notes.len(),
            "blockerCount":blockers},"error":null})
}

/// One advisor note as `[severity] [advisor] note`, and whether it is a blocker.
fn advisor_note(value: &Value) -> Option<(String, bool)> {
    let note = trimmed(&value["note"])?;
    let severity = value["severity"]
        .as_str()
        .filter(|severity| matches!(*severity, "nit" | "concern" | "blocker"));
    let tags: Vec<String> = [severity, trimmed(&value["advisor"])]
        .into_iter()
        .flatten()
        .map(|tag| format!("[{tag}]"))
        .collect();
    let line = if tags.is_empty() {
        note.to_owned()
    } else {
        format!("{} {note}", tags.join(" "))
    };
    Some((line, severity == Some("blocker")))
}

/// A `<system-notice>` as a notification, as Paseo's `mapOmpSystemNoticeToNotification`: a
/// background `<task-result>` is "Background job <id> <status>" at its status's level,
/// otherwise the notice's first line. The notice's remaining lines follow, so the job output
/// that Ait's former background card showed stays visible.
fn system_notice(text: &str) -> Option<Value> {
    if !text.trim_start().starts_with(NOTICE_OPEN) {
        return None;
    }
    let open = text.find(NOTICE_OPEN).unwrap_or_default() + NOTICE_OPEN.len();
    let body = &text[open..];
    let body = body.find(NOTICE_CLOSE).map_or(body, |close| &body[..close]);
    let mut lines = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('<'));
    let first = lines.next();
    let task = task_result(text);
    let status = task.as_ref().and_then(|task| task.status.as_deref());
    let level = match status.map(str::to_ascii_lowercase).as_deref() {
        Some("failed" | "error") => "error",
        Some("canceled" | "cancelled" | "stopped") => "warning",
        _ => "info",
    };
    let label = match task.as_ref().and_then(|task| task.id.as_deref()) {
        Some(id) => format!("Background job {id} {}", status.unwrap_or("completed")),
        None => first.unwrap_or("System notice").to_owned(),
    };
    let rest: Vec<&str> = lines.collect();
    let message = if rest.is_empty() {
        label
    } else {
        format!("{label}\n{}", rest.join("\n"))
    };
    Some(json!({"type":"notification","level":level,"message":preview(&message)}))
}

/// The `id` and `status` attributes of a notice's `<task-result …>` tag.
#[derive(Debug, Default)]
struct TaskResult {
    id: Option<String>,
    status: Option<String>,
}

/// Read `<task-result id="…" status="…">`, accepting straight and typographic quotes, as
/// Paseo's `readTaskResult`.
fn task_result(text: &str) -> Option<TaskResult> {
    const TAG: &str = "<task-result";
    let start = text.to_ascii_lowercase().find(TAG)? + TAG.len();
    let rest = &text[start..];
    if rest
        .chars()
        .next()
        .is_some_and(|next| next.is_alphanumeric() || next == '_')
    {
        return None;
    }
    let attributes = &rest[..rest.find('>')?];
    let mut result = TaskResult::default();
    for (name, value) in tag_attributes(attributes) {
        let value = Some(value.trim().to_owned()).filter(|value| !value.is_empty());
        match name {
            "id" => result.id = value,
            "status" => result.status = value,
            _ => {}
        }
    }
    Some(result)
}

/// The `name="value"` pairs of a tag, as Paseo's attribute pattern
/// `([\w-]+)=["'“‘]([^"'“”‘’]*)["'”’]`.
fn tag_attributes(attributes: &str) -> Vec<(&str, &str)> {
    const OPENING: [char; 4] = ['"', '\'', '“', '‘'];
    const CLOSING: [char; 6] = ['"', '\'', '“', '”', '‘', '’'];
    let mut pairs = Vec::new();
    let mut rest = attributes;
    while let Some(equals) = rest.find('=') {
        let name_start = rest[..equals]
            .rfind(|character: char| !(character.is_alphanumeric() || "_-".contains(character)))
            .map_or(0, |index| {
                index + rest[index..].chars().next().map_or(1, char::len_utf8)
            });
        let name = &rest[name_start..equals];
        let after = &rest[equals + 1..];
        let Some(quote) = after.chars().next().filter(|quote| OPENING.contains(quote)) else {
            rest = after;
            continue;
        };
        let value_start = quote.len_utf8();
        let Some(length) = after[value_start..].find(CLOSING) else {
            break;
        };
        if !name.is_empty() {
            pairs.push((name, &after[value_start..value_start + length]));
        }
        rest = &after[value_start + length..];
        rest = &rest[rest.chars().next().map_or(0, char::len_utf8)..];
    }
    pairs
}

/// Project an OMP runtime telemetry `event` (M2, M3) into its timeline item, as Paseo's
/// `mapOmpRuntimeEventToTimelineItem`, plus OMP's `compaction_*` and 18.18.8's
/// `manual_compaction_*` lifecycle. Returns none for other or malformed events.
pub(super) fn telemetry(event: &Value) -> Option<Value> {
    match event["type"].as_str()? {
        "notice" => notice(event),
        "goal_updated" => Some(goal(event)),
        "auto_retry_start" => retry_start(event),
        "auto_retry_end" => retry_end(event),
        "retry_fallback_applied" => fallback_applied(event),
        "retry_fallback_succeeded" => fallback_succeeded(event),
        "auto_compaction_start" => Some(compaction(false, None, Some("auto"))),
        "auto_compaction_end" => Some(compaction(true, Some(&event["result"]), Some("auto"))),
        "compaction_start" | "compaction_end" => {
            let trigger = if event["reason"] == "manual" {
                "manual"
            } else {
                "auto"
            };
            Some(compaction(
                event["type"] == "compaction_end",
                None,
                Some(trigger),
            ))
        }
        "manual_compaction_start" => Some(compaction(false, None, Some("manual"))),
        "manual_compaction_end" => Some(compaction(true, Some(&event["result"]), Some("manual"))),
        _ => None,
    }
}

/// Whether `event` is a compaction lifecycle event that an Ait-issued `/compact` reports
/// with its own rows instead (M3).
pub(super) fn manual_compaction(event: &Value) -> bool {
    matches!(
        event["type"].as_str(),
        Some(
            "compaction_start"
                | "compaction_end"
                | "manual_compaction_start"
                | "manual_compaction_end"
        )
    )
}

fn notice(event: &Value) -> Option<Value> {
    let level = event["level"]
        .as_str()
        .filter(|level| matches!(*level, "info" | "warning" | "error"))?;
    let message = event["message"].as_str()?;
    Some(json!({"type":"notification","level":level,"message":preview(message)}))
}

fn goal(event: &Value) -> Value {
    let goal = [&event["goal"], &event["state"]["goal"]]
        .into_iter()
        .find(|goal| goal.is_object());
    let id = goal.and_then(|goal| goal["id"].as_str());
    let status = goal.and_then(|goal| goal["status"].as_str());
    let text = goal.map_or_else(
        || "OMP goal cleared.".to_owned(),
        |goal| goal_text(event, goal),
    );
    let mut metadata = json!({"synthetic":true,"source":"omp_goal_updated"});
    if let Some(id) = id {
        metadata["goalId"] = json!(id);
    }
    if let Some(status) = status {
        metadata["goalStatus"] = json!(status);
    }
    let label = status.map_or_else(
        || "OMP goal updated".to_owned(),
        |status| format!("OMP goal {status}"),
    );
    status_row(Row {
        call_id: format!(
            "omp-goal:{}",
            id.map_or_else(|| digest(&[&text]), str::to_owned)
        ),
        name: "omp_goal_updated",
        label,
        text: Some(text),
        icon: "brain",
        status: "completed",
        error: None,
        metadata,
    })
}

fn goal_text(event: &Value, goal: &Value) -> String {
    let objective = goal["objective"]
        .as_str()
        .map(str::trim)
        .filter(|objective| !objective.is_empty())
        .unwrap_or("OMP goal updated.");
    let mut parts = vec![objective.to_owned()];
    if let Some(status) = goal["status"].as_str() {
        parts.push(format!("Status: {status}"));
    }
    for (field, label, unit) in [
        ("tokensUsed", "Tokens used", ""),
        ("tokenBudget", "Token budget", ""),
        ("timeUsedSeconds", "Time used", "s"),
    ] {
        if goal[field].is_number() {
            parts.push(format!("{label}: {}{unit}", goal[field]));
        }
    }
    if let Some(mode) = event["state"]["mode"]
        .as_str()
        .filter(|mode| !mode.is_empty())
    {
        parts.push(format!("Mode: {mode}"));
    }
    parts.join("\n")
}

fn retry_start(event: &Value) -> Option<Value> {
    let attempt = event["attempt"].as_u64()?;
    let maximum = event["maxAttempts"]
        .as_u64()
        .filter(|maximum| *maximum > 0)?;
    let delay = event["delayMs"].as_u64()?;
    let error = event["errorMessage"].as_str()?;
    let mut metadata = json!({"synthetic":true,"source":"omp_auto_retry_start",
        "attempt":attempt,"maxAttempts":maximum,"delayMs":delay});
    if event["errorId"].is_i64() {
        metadata["errorId"] = event["errorId"].clone();
    }
    Some(status_row(Row {
        call_id: format!("omp-auto-retry:{attempt}"),
        name: "omp_auto_retry",
        label: format!("OMP retry {attempt}/{maximum}"),
        text: Some(format!("Retrying in {}: {error}", delay_text(delay))),
        icon: "sparkles",
        status: "running",
        error: None,
        metadata,
    }))
}

fn retry_end(event: &Value) -> Option<Value> {
    let attempt = event["attempt"].as_u64()?;
    let success = event["success"].as_bool()?;
    let final_error = event["finalError"].as_str();
    let (label, status, fallback) = if success {
        ("recovered", "completed", "Retry recovered.")
    } else {
        ("failed", "failed", "Retry failed.")
    };
    Some(status_row(Row {
        call_id: format!("omp-auto-retry:{attempt}"),
        name: "omp_auto_retry",
        label: format!("OMP retry {attempt} {label}"),
        text: Some(final_error.unwrap_or(fallback).to_owned()),
        icon: "sparkles",
        status,
        error: (!success).then(|| final_error.unwrap_or("OMP retry failed").to_owned()),
        metadata: json!({"synthetic":true,"source":"omp_auto_retry_end","attempt":attempt,
            "success":success}),
    }))
}

fn fallback_applied(event: &Value) -> Option<Value> {
    let role = event["role"].as_str()?;
    let from = event["from"].as_str()?;
    let to = event["to"].as_str()?;
    Some(status_row(Row {
        call_id: format!("omp-retry-fallback:{}", digest(&[role, from, to])),
        name: "omp_retry_fallback",
        label: format!("OMP fallback applied for {role}"),
        text: Some(format!("{from} -> {to}")),
        icon: "sparkles",
        status: "completed",
        error: None,
        metadata: json!({"synthetic":true,"source":"omp_retry_fallback_applied","role":role,
            "from":from,"to":to}),
    }))
}

fn fallback_succeeded(event: &Value) -> Option<Value> {
    let role = event["role"].as_str()?;
    let model = event["model"].as_str()?;
    Some(status_row(Row {
        call_id: format!("omp-retry-fallback-succeeded:{}", digest(&[role, model])),
        name: "omp_retry_fallback",
        label: format!("OMP fallback succeeded for {role}"),
        text: Some(format!("Using {model}")),
        icon: "sparkles",
        status: "completed",
        error: None,
        metadata: json!({"synthetic":true,"source":"omp_retry_fallback_succeeded","role":role,
            "model":model}),
    }))
}

/// A recorded `compaction` entry as Paseo's `mapCompactionEntry` shows it: a completed row
/// with the entry's `tokensBefore` as `preTokens`. The entry does not record its trigger.
pub(super) fn compaction_entry(entry: &Value) -> Value {
    compaction(true, Some(entry), None)
}

/// A compaction row; an end reports `tokensBefore` from its result as `preTokens`. Every end
/// completes the row, as Paseo does, since a loading row must always clear.
pub(super) fn compaction(end: bool, result: Option<&Value>, trigger: Option<&str>) -> Value {
    let mut item = json!({"type":"compaction","status":if end {"completed"} else {"loading"}});
    if let Some(trigger) = trigger {
        item["trigger"] = json!(trigger);
    }
    if let Some(tokens) = result.filter(|result| result["tokensBefore"].is_number()) {
        item["preTokens"] = tokens["tokensBefore"].clone();
    }
    item
}

/// A status-line tool row for one telemetry event, as Paseo's `buildStatusLineItem`.
struct Row {
    call_id: String,
    name: &'static str,
    label: String,
    text: Option<String>,
    icon: &'static str,
    status: &'static str,
    error: Option<String>,
    metadata: Value,
}

fn status_row(row: Row) -> Value {
    let mut detail = json!({"type":"plain_text","label":row.label,"icon":row.icon});
    if let Some(text) = row.text.filter(|text| !text.is_empty()) {
        detail["text"] = json!(preview(&text));
    }
    let error = match (row.status, row.error) {
        ("failed", Some(error)) => json!(error),
        ("failed", None) => detail["label"].clone(),
        _ => Value::Null,
    };
    json!({"type":"tool_call","callId":row.call_id,"name":row.name,"status":row.status,
        "detail":detail,"metadata":row.metadata,"error":error})
}

/// Paseo's `formatDelay`: milliseconds below a second, else whole or tenths of seconds.
fn delay_text(milliseconds: u64) -> String {
    if milliseconds < 1000 {
        format!("{milliseconds}ms")
    } else if milliseconds.is_multiple_of(1000) {
        format!("{}s", milliseconds / 1000)
    } else {
        let tenths = (milliseconds + 50) / 100;
        format!("{}.{}s", tenths / 10, tenths % 10)
    }
}

/// A todo list from OMP todo phases (`[{name, tasks:[…]}]`), as the todo tool's
/// `details.phases` and `get_state.todoPhases` report them.
pub(super) fn todo_phases(phases: &Value) -> Option<Value> {
    let phases = phases.as_array().filter(|phases| phases.len() <= 1024)?;
    let mut items = Vec::new();
    let mut total_tasks = 0_usize;
    let mut source_bytes = 0_usize;
    for (index, phase) in phases.iter().enumerate() {
        let title = phase["name"].as_str().filter(|title| title.len() <= 8192)?;
        let tasks = phase["tasks"].as_array()?;
        total_tasks = total_tasks.saturating_add(tasks.len());
        source_bytes = source_bytes.saturating_add(title.len().saturating_mul(tasks.len()));
        if total_tasks > 1024 || source_bytes > 128 * 1024 {
            return None;
        }
        for task in tasks {
            source_bytes = source_bytes.saturating_add(task["content"].as_str()?.len());
            source_bytes =
                source_bytes.saturating_add(task["blocker"].as_str().map_or(0, str::len));
        }
        if total_tasks > 1024 || source_bytes > 128 * 1024 {
            return None;
        }
        let mut projected = todo_items(tasks.iter().collect())?;
        let tasks = projected["items"].as_array_mut()?;
        for task in tasks.iter_mut() {
            task["group"] = json!({"id":format!("phase:{index}"),"title":title});
        }
        items.append(tasks);
    }
    let item = json!({"type":"todo","items":items});
    (serde_json::to_vec(&item).ok()?.len() <= 128 * 1024).then_some(item)
}

/// A todo list from a `todo_reminder` event's open `todos`.
pub(super) fn todo_reminder(event: &Value) -> Option<Value> {
    let todos = event["todos"]
        .as_array()
        .filter(|items| items.len() <= 1024)?;
    todo_items(todos.iter().collect())
}

/// Paseo's `mapOmpTodoItems`: abandoned items are dropped and blocked items name their
/// blocker. An empty list clears earlier tasks; malformed lists are ignored.
fn todo_items(tasks: Vec<&Value>) -> Option<Value> {
    if tasks.len() > 1024 {
        return None;
    }
    let mut items = Vec::with_capacity(tasks.len());
    let mut source_bytes = 0_usize;
    for task in tasks {
        let content = task["content"].as_str().filter(|text| text.len() <= 8192)?;
        let blocker = task["blocker"].as_str().unwrap_or_default();
        source_bytes = source_bytes
            .saturating_add(content.len())
            .saturating_add(blocker.len());
        if blocker.len() > 8192 || source_bytes > 128 * 1024 {
            return None;
        }
        let status = task["status"].as_str()?;
        let (text, status) = match status {
            "abandoned" => continue,
            "blocked" => {
                let reason = task["blocker"]
                    .as_str()
                    .filter(|blocker| !blocker.is_empty())
                    .map_or_else(
                        || "blocked".to_owned(),
                        |blocker| format!("blocked: {blocker}"),
                    );
                (format!("{content} ({reason})"), "pending")
            }
            "pending" | "in_progress" | "completed" => (content.to_owned(), status),
            _ => return None,
        };
        items.push(json!({"text":text,"status":status,"completed":status == "completed"}));
    }
    let item = json!({"type":"todo","items":items});
    (serde_json::to_vec(&item).ok()?.len() <= 128 * 1024).then_some(item)
}

/// A short stable hexadecimal digest of `parts`, for call ids without a native identity.
fn digest(parts: &[&str]) -> String {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update(part.as_bytes());
        hash.update([0]);
    }
    let digest = format!("{:x}", hash.finalize());
    digest[..12].to_owned()
}

fn plural<'a>(count: usize, one: &'a str, many: &'a str) -> &'a str {
    if count == 1 { one } else { many }
}

fn trimmed(value: &Value) -> Option<&str> {
    value
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

#[cfg(test)]
mod tests;
