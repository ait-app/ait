//! Durable execution evidence is separate from prompt admission and message completion.
use crate::local::opencode::types::Outcome;
use crate::local::opencode::types::{Fault, ProtocolError};
use serde_json::Value;

use super::{Version, failure};

pub(super) fn outcome(
    version: Version,
    info: &Value,
    execution: Option<&Value>,
    history: &[Value],
) -> Result<Option<Outcome>, ProtocolError> {
    if version == Version::V1 {
        // An earlier assistant cannot settle a newer queued input.
        let Some(last) = history.last().and_then(|message| message.get("info")) else {
            return Ok(None);
        };
        return Ok(
            (last.get("role").and_then(Value::as_str) == Some("assistant")).then(|| {
                if last.get("error").is_some_and(|error| !error.is_null()) {
                    Outcome::Failed
                } else {
                    Outcome::Completed
                }
            }),
        );
    }
    let Some(event) = execution else {
        return idle_outcome(info, history);
    };
    let created = event
        .get("created")
        .and_then(Value::as_i64)
        .ok_or_else(|| {
            failure(
                Fault::ProviderFailed,
                "OpenCode execution timestamp missing",
            )
        })?;
    if history
        .iter()
        .rev()
        .find(|message| message.get("type").and_then(Value::as_str) == Some("user"))
        .and_then(|message| message.pointer("/time/created"))
        .and_then(Value::as_i64)
        .is_some_and(|input| created < input)
    {
        return Ok(None);
    }
    let (outcome, expected) = match event.get("type").and_then(Value::as_str) {
        Some("session.execution.succeeded") => (Outcome::Completed, "succeeded"),
        Some("session.execution.failed") => (Outcome::Failed, "failed"),
        Some("session.execution.interrupted") => {
            if event.pointer("/data/reason").and_then(Value::as_str) == Some("shutdown") {
                return Ok(None);
            }
            (Outcome::Interrupted, "interrupted")
        }
        Some("session.execution.started") => return Ok(None),
        Some(_) | None => {
            return Err(failure(
                Fault::ProviderFailed,
                "invalid OpenCode execution event",
            ));
        }
    };
    if info.get("outcome").and_then(Value::as_str) != Some(expected) {
        return Err(failure(
            Fault::RunRecoveryFailed,
            "OpenCode execution outcome has not reconciled",
        ));
    }
    Ok(Some(outcome))
}

fn idle_outcome(info: &Value, history: &[Value]) -> Result<Option<Outcome>, ProtocolError> {
    if interrupted_assistant(history) {
        return Ok(Some(Outcome::Interrupted));
    }
    // V2.0.20 filters execution events out of the public log. Its persisted idle row
    // is the completion boundary; an old assistant or session-level outcome alone is not.
    let Some(last) = history.last().filter(|row| row["type"] == "idle") else {
        return Ok(None);
    };
    let created = last.pointer("/time/created").and_then(Value::as_i64);
    let input = history
        .iter()
        .rev()
        .find(|row| row["type"] == "user")
        .and_then(|row| row.pointer("/time/created"))
        .and_then(Value::as_i64);
    if !matches!((created, input), (Some(end), Some(start)) if end >= start) {
        return Err(failure(
            Fault::RunRecoveryFailed,
            "invalid OpenCode idle boundary",
        ));
    }
    let outcome = match last["outcome"].as_str() {
        Some("succeeded") => Outcome::Completed,
        Some("failed") => Outcome::Failed,
        Some("interrupted") => Outcome::Interrupted,
        _ => {
            return Err(failure(
                Fault::ProviderFailed,
                "invalid OpenCode idle outcome",
            ));
        }
    };
    if info.get("outcome") != last.get("outcome") {
        return Err(failure(
            Fault::RunRecoveryFailed,
            "OpenCode idle outcome has not reconciled",
        ));
    }
    Ok(Some(outcome))
}

fn interrupted_assistant(history: &[Value]) -> bool {
    // V2.0.20 aborts a declined tool without an idle row or a session outcome.
    // snapshot() brackets this evidence with native inactivity checks.
    let Some(last) = history.last() else {
        return false;
    };
    if last["type"] != "assistant"
        || last["finish"] != "error"
        || last.pointer("/error/type").and_then(Value::as_str) != Some("aborted")
    {
        return false;
    }
    let input = history
        .iter()
        .rev()
        .find(|row| row["type"] == "user")
        .and_then(|row| row.pointer("/time/created"))
        .and_then(Value::as_i64);
    let created = last.pointer("/time/created").and_then(Value::as_i64);
    let completed = last.pointer("/time/completed").and_then(Value::as_i64);
    matches!((input, created, completed), (Some(input), Some(created), Some(completed))
        if input <= created && created <= completed)
}

#[cfg(test)]
mod tests;
