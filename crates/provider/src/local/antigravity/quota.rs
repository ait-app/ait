//! Native account quota queries, separate from a conversation's cumulative token usage.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::Path;

use serde_json::{Value, json};

use super::{AntigravityClient, PROVIDER, discovery, transport};
use crate::ports::agent_session::AgentSessionError;

/// Read quotas with the client's native launch environment and the supplied working directory.
///
/// Returns only quota display fields. Spawn, timeout, native failure and malformed output errors
/// use `AgentSessionError`; no credentials or raw native diagnostics leave this adapter.
pub(super) async fn read(
    client: &AntigravityClient,
    cwd: &Path,
) -> Result<Value, AgentSessionError> {
    // Native model listing only uses cached authentication. Print mode can launch OAuth even
    // with stdin closed, so refuse signed-out background refreshes before invoking /usage.
    discovery::models(client, cwd.to_str().ok_or(AgentSessionError::Failed)?).await?;
    // This built-in command returns quota JSON with zero turns and no conversation. Keep it
    // separate from the session stream, where slash expansion is intentionally disabled.
    let bytes = transport::query(
        client,
        cwd,
        &["--print", "/usage", "--output-format", "json"].map(OsStr::new),
    )
    .await?;
    let response = serde_json::from_slice(&bytes).map_err(|_| AgentSessionError::Failed)?;
    project(&response)
}

fn project(response: &Value) -> Result<Value, AgentSessionError> {
    if response["status"] != "SUCCESS"
        || response["command"]["name"] != "usage"
        || response["num_turns"].as_u64() != Some(0)
    {
        return Err(AgentSessionError::Failed);
    }
    let groups = response["command"]["data"]["groups"]
        .as_array()
        .filter(|groups| groups.len() <= 64)
        .ok_or(AgentSessionError::Failed)?;
    let mut ids = BTreeSet::new();
    let mut windows = Vec::new();
    for group in groups {
        let name = text(&group["name"])?;
        let buckets = group["buckets"]
            .as_array()
            .filter(|buckets| buckets.len() <= 512 - windows.len())
            .ok_or(AgentSessionError::Failed)?;
        for bucket in buckets {
            let id = text(&bucket["id"])?;
            if !ids.insert(id) {
                return Err(AgentSessionError::Failed);
            }
            windows.push(window(name, id, bucket)?);
        }
    }
    let empty = windows.is_empty();
    let mut report = json!({
        "providerId":PROVIDER,"displayName":"Antigravity",
        "status":if empty {"unavailable"} else {"available"},
        "planLabel":null,"windows":windows,"balances":[],"details":[],"error":null,
        "sourceLabel":"Antigravity CLI /usage",
        "fetchedAt":chrono::Utc::now().to_rfc3339(),"nextRefreshAt":null
    });
    if empty {
        report["problem"] = json!({"kind":"no_quota",
            "detail":"Antigravity CLI did not report quota windows for this account."});
    }
    Ok(report)
}

fn window(group: &str, id: &str, bucket: &Value) -> Result<Value, AgentSessionError> {
    let period = text(&bucket["window"])?;
    let remaining = bucket["remaining_fraction"]
        .as_f64()
        .filter(|fraction| fraction.is_finite() && (0.0..=1.0).contains(fraction))
        .ok_or(AgentSessionError::Failed)?
        * 100.0;
    let reset = if bucket["reset_time"].is_null() {
        None
    } else {
        let reset = text(&bucket["reset_time"])?;
        chrono::DateTime::parse_from_rfc3339(reset).map_err(|_| AgentSessionError::Failed)?;
        Some(reset)
    };
    let (label, short) = match period {
        "5h" => ("5 hours", "5h"),
        "weekly" => ("Weekly", "wk"),
        other => (other, other),
    };
    let used = 100.0 - remaining;
    Ok(
        json!({"id":id,"label":format!("{group} · {label}"),"shortLabel":short,
        "summary":period == "5h","usedPct":used,"remainingPct":remaining,"resetsAt":reset,
        "tone":if used > 90.0 {"danger"} else if used >= 70.0 {"warning"} else {"ok"}}),
    )
}

fn text(value: &Value) -> Result<&str, AgentSessionError> {
    value
        .as_str()
        .filter(|text| {
            !text.trim().is_empty() && text.len() <= 256 && !text.chars().any(char::is_control)
        })
        .ok_or(AgentSessionError::Failed)
}

#[cfg(test)]
mod tests;
