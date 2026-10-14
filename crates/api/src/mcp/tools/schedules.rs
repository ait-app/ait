//! Cron and heartbeat translation retaining Ait's Workspace-owned scheduler.

use chrono::{TimeDelta, Utc};
use model::ErrorCode;
use serde_json::{Value, json};

use crate::Shared;
use crate::connection::rpc;

use super::{creation, rename, take};
use crate::mcp::catalog::Tool;

/// Create a Workspace-owned schedule from `arguments` and optional `caller` in `state`.
/// `heartbeat` selects the calling Agent as target instead of a new Agent per occurrence.
/// # Errors
/// Returns invalid context, duration, provider settings or existing scheduler failures.
pub(super) async fn create(
    state: &Shared,
    mut arguments: Value,
    caller: Option<&Value>,
    heartbeat: bool,
) -> Result<Value, ErrorCode> {
    if arguments.get("workspaceId").is_none() {
        arguments["workspaceId"] = caller
            .and_then(|caller| caller.get("workspaceId"))
            .cloned()
            .ok_or(ErrorCode::InvalidMessage)?;
    }
    let target = if heartbeat {
        json!({"type":"agent","agentId":caller.ok_or(ErrorCode::InvalidMessage)?["id"]})
    } else {
        if arguments.get("provider").is_none() {
            arguments["provider"] = caller
                .and_then(|caller| caller.get("provider"))
                .cloned()
                .ok_or(ErrorCode::InvalidMessage)?;
        }
        creation::directory(state, &mut arguments, caller).await?;
        let provider = take(&mut arguments, "provider")?.ok_or(ErrorCode::InvalidMessage)?;
        let (provider, model) = creation::provider_model(&provider, false)?;
        let mut config = json!({"provider":provider,"cwd":take(&mut arguments,"cwd")?});
        if let Some(model) = model {
            config["model"] = json!(model);
        }
        json!({"type":"new-agent","config":config})
    };
    let cron = take(&mut arguments, "cron")?.ok_or(ErrorCode::InvalidMessage)?;
    let mut cadence = json!({"type":"cron","expression":cron});
    if let Some(timezone) = take(&mut arguments, "timezone")? {
        cadence["timezone"] = timezone;
    }
    if let Some(expiry) = take(&mut arguments, "expiresIn")? {
        arguments["expiresAt"] = expiry_at(&expiry)?;
    }
    arguments["cadence"] = cadence;
    arguments["target"] = target;
    rpc::execute(state, "schedule.create.request", arguments).await
}

/// Return new-Agent schedules from `state`, excluding heartbeat targets.
/// # Errors
/// Propagates existing scheduler read failures.
pub(super) async fn list(state: &Shared) -> Result<Value, ErrorCode> {
    let mut value = rpc::execute(state, "schedule.list.request", json!({})).await?;
    if let Some(schedules) = value["schedules"].as_array_mut() {
        schedules.retain(|schedule| schedule["target"]["type"] == "new-agent");
    }
    Ok(value)
}

/// Execute schedule `tool` with `arguments` in `state`, checking `caller` heartbeat ownership.
/// # Errors
/// Rejects incorrect target kinds, conflicting patches or existing scheduler failures.
pub(super) async fn operate(
    state: &Shared,
    tool: &Tool,
    mut arguments: Value,
    caller: Option<&Value>,
) -> Result<Value, ErrorCode> {
    rename(&mut arguments, "id", "scheduleId")?;
    let snapshot = rpc::execute(
        state,
        "schedule.inspect.request",
        json!({"scheduleId":arguments["scheduleId"]}),
    )
    .await?;
    let schedule = &snapshot["schedule"];
    if tool.name == "delete_heartbeat" {
        if schedule["target"]["type"] != "agent"
            || caller.is_none_or(|caller| schedule["target"]["agentId"] != caller["id"])
        {
            return Err(ErrorCode::InvalidMessage);
        }
    } else if schedule["target"]["type"] != "new-agent" {
        return Err(ErrorCode::InvalidMessage);
    }
    if tool.name == "inspect_schedule" {
        return Ok(snapshot);
    }
    if tool.name == "update_schedule" {
        arguments = update(arguments)?;
    }
    let limit = take(&mut arguments, "limit")?;
    let method = if tool.name == "delete_heartbeat" {
        "schedule.delete.request"
    } else {
        &tool.methods[0]
    };
    let mut result = rpc::execute(state, method, arguments).await?;
    if tool.name == "schedule_logs"
        && let Some(runs) = result["runs"].as_array_mut()
    {
        let limit = limit.as_ref().and_then(Value::as_u64).unwrap_or(20);
        let start = runs
            .len()
            .saturating_sub(usize::try_from(limit).unwrap_or(200));
        runs.drain(..start);
    }
    Ok(result)
}

fn update(mut arguments: Value) -> Result<Value, ErrorCode> {
    let cron = take(&mut arguments, "cron")?;
    let every = take(&mut arguments, "every")?;
    let timezone = take(&mut arguments, "timezone")?;
    match (cron, every) {
        (Some(cron), None) => {
            arguments["cadence"] = json!({"type":"cron","expression":cron});
            if let Some(timezone) = timezone {
                arguments["cadence"]["timezone"] = timezone;
            }
        }
        (None, Some(every)) if timezone.is_none() => {
            arguments["cadence"] = json!({"type":"every","everyMs":duration_ms(&every)?});
        }
        (None, None) if timezone.is_none() => {}
        _ => return Err(ErrorCode::InvalidMessage),
    }
    let expiry = take(&mut arguments, "expiresIn")?;
    let clear = take(&mut arguments, "clearExpires")? == Some(json!(true));
    if expiry.is_some() && clear {
        return Err(ErrorCode::InvalidMessage);
    }
    if let Some(expiry) = expiry {
        arguments["expiresAt"] = expiry_at(&expiry)?;
    } else if clear {
        arguments["expiresAt"] = Value::Null;
    }
    let mut config = json!({});
    if let Some(value) = take(&mut arguments, "provider")? {
        let (provider, model) = creation::provider_model(&value, false)?;
        config["provider"] = json!(provider);
        if let Some(model) = model {
            config["model"] = json!(model);
        }
    }
    if let Some(model) = take(&mut arguments, "model")? {
        if config.get("model").is_some_and(|current| current != &model) {
            return Err(ErrorCode::InvalidMessage);
        }
        config["model"] = model;
    }
    for (old, new) in [("cwd", "cwd"), ("mode", "modeId")] {
        if let Some(value) = take(&mut arguments, old)? {
            config[new] = value;
        }
    }
    if config.as_object().is_some_and(|config| !config.is_empty()) {
        arguments["newAgentConfig"] = config;
    }
    Ok(arguments)
}

fn expiry_at(value: &Value) -> Result<Value, ErrorCode> {
    let delta =
        TimeDelta::try_milliseconds(duration_ms(value)?).ok_or(ErrorCode::InvalidMessage)?;
    let time = Utc::now()
        .checked_add_signed(delta)
        .ok_or(ErrorCode::InvalidMessage)?;
    Ok(json!(time.to_rfc3339()))
}

fn duration_ms(value: &Value) -> Result<i64, ErrorCode> {
    let text = value.as_str().ok_or(ErrorCode::InvalidMessage)?.trim();
    if text.is_empty() {
        return Err(ErrorCode::InvalidMessage);
    }
    let mut total = 0_i64;
    let mut number = 0_i64;
    let mut has_digits = false;
    let mut has_units = false;
    for byte in text.bytes() {
        if byte.is_ascii_digit() {
            number = number
                .checked_mul(10)
                .and_then(|number| number.checked_add(i64::from(byte - b'0')))
                .ok_or(ErrorCode::InvalidMessage)?;
            has_digits = true;
            continue;
        }
        if !has_digits {
            return Err(ErrorCode::InvalidMessage);
        }
        let multiplier = match byte {
            b's' => 1000,
            b'm' => 60_000,
            b'h' => 3_600_000,
            _ => return Err(ErrorCode::InvalidMessage),
        };
        total = number
            .checked_mul(multiplier)
            .and_then(|part| total.checked_add(part))
            .ok_or(ErrorCode::InvalidMessage)?;
        number = 0;
        has_digits = false;
        has_units = true;
    }
    if has_digits {
        if has_units {
            return Err(ErrorCode::InvalidMessage);
        }
        total = number.checked_mul(1000).ok_or(ErrorCode::InvalidMessage)?;
    }
    if total <= 0 {
        return Err(ErrorCode::InvalidMessage);
    }
    Ok(total)
}

#[cfg(test)]
mod tests;
