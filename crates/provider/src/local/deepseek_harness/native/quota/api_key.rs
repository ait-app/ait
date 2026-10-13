//! Official `DeepSeek` balance API, using the key selected by DSH's native credential source.
use std::{collections::BTreeSet, time::Duration};

use futures_util::StreamExt;
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};

use crate::ports::agent_session::AgentSessionError;

/// Read only the fixed official balance endpoint; never follow credential-bearing redirects.
/// Returns safe HTTP authentication problems or bounded transport/schema failures.
pub(super) async fn read(key: &SecretString) -> Result<Value, AgentSessionError> {
    fetch(
        key,
        "https://api.deepseek.com/user/balance",
        Duration::from_secs(20),
    )
    .await
}

async fn fetch(
    key: &SecretString,
    endpoint: &str,
    timeout: Duration,
) -> Result<Value, AgentSessionError> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .build()
        .map_err(|_| AgentSessionError::Failed)?;
    let response = client
        .get(endpoint)
        .bearer_auth(key.expose_secret())
        .send()
        .await
        .map_err(|_| AgentSessionError::Failed)?;
    let status = response.status().as_u16();
    if matches!(status, 401 | 403) {
        let mut report = super::unavailable("DeepSeek rejected the API key configured in DSH.");
        report["problem"] =
            json!({"kind":"rejected","status":status,"refreshedBy":"DSH Models settings"});
        return Ok(report);
    }
    if !response.status().is_success() {
        return Err(AgentSessionError::Failed);
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| AgentSessionError::Failed)?;
        if bytes.len().saturating_add(chunk.len()) > 65536 {
            return Err(AgentSessionError::Failed);
        }
        bytes.extend_from_slice(&chunk);
    }
    project(&serde_json::from_slice(&bytes).map_err(|_| AgentSessionError::Failed)?)
}

fn project(response: &Value) -> Result<Value, AgentSessionError> {
    let available = response["is_available"]
        .as_bool()
        .ok_or(AgentSessionError::Failed)?;
    let wallets = response["balance_infos"]
        .as_array()
        .filter(|wallets| wallets.len() <= 2)
        .ok_or(AgentSessionError::Failed)?;
    let mut balances = Vec::with_capacity(wallets.len() * 3);
    let mut currencies = BTreeSet::new();
    for wallet in wallets {
        let unit = match wallet["currency"].as_str() {
            Some("CNY") => "cny",
            Some("USD") => "usd",
            _ => return Err(AgentSessionError::Failed),
        };
        if !currencies.insert(unit) {
            return Err(AgentSessionError::Failed);
        }
        for (field, id, label) in [
            ("total_balance", "total", "DeepSeek total balance"),
            ("topped_up_balance", "recharge", "DeepSeek recharge balance"),
            ("granted_balance", "bonus", "DeepSeek bonus balance"),
        ] {
            balances.push(json!({"id":format!("{id}-{unit}"),"label":label,
                "remaining":super::amount(&wallet[field])?,"unit":unit,"tone":"default"}));
        }
    }
    let mut report = if balances.is_empty() {
        super::unavailable("DeepSeek did not report balances for this API key.")
    } else {
        super::report(&balances)
    };
    report["sourceLabel"] = json!("DSH DeepSeek API balance");
    report["details"] = json!([{"id":"api-available","label":"DeepSeek API calls",
        "value":if available {"Balance available"} else {"Insufficient balance"},
        "tone":if available {"default"} else {"warning"}}]);
    Ok(report)
}

#[cfg(test)]
mod tests;
