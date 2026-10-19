//! DSH-selected `DeepSeek` balances; authentication material never enters public reports.

use std::collections::BTreeSet;

use secrecy::ExposeSecret;
use serde_json::{Value, json};

use crate::ports::account_usage::{PreparedUsage, UsageIdentity};

use super::{http::Api, runtime::Runtime};
use crate::{
    local::deepseek_harness::{DeepSeekHarnessClient, PROVIDER},
    ports::agent_session::AgentSessionError,
};

mod api_key;
pub(super) mod credentials;

/// Query the launch account without creating a native session or sending model input.
/// Returns bounded native transport/schema errors and always closes the temporary Host.
pub(in crate::local::deepseek_harness) async fn read(
    client: &DeepSeekHarnessClient,
    cwd: &str,
) -> Result<Value, AgentSessionError> {
    prepare_host(client, cwd).await?.report.await
}

/// Resolve a temporary Host's account before looking up cached quotas.
/// The temporary process always closes before the lazy quota request is returned.
/// # Errors
/// Returns bounded native transport or credential-resolution errors.
pub(in crate::local::deepseek_harness) async fn prepare_host(
    client: &DeepSeekHarnessClient,
    cwd: &str,
) -> Result<PreparedUsage<'static>, AgentSessionError> {
    let context = credentials::Context::capture(client, cwd).await;
    let mut runtime = Runtime::open(client, cwd).await?;
    let result = prepare(&runtime.api, &context).await;
    runtime.close().await?;
    result
}

/// Read only the `DeepSeek` account owned by this Host, independent of its selected model.
/// # Errors
/// Returns safe credential, transport or schema failures.
pub(super) async fn query(
    api: &Api,
    context: &Result<credentials::Context, AgentSessionError>,
) -> Result<Value, AgentSessionError> {
    prepare(api, context).await?.report.await
}

/// Resolve credentials once so cached and historical facts cannot cross account changes.
/// # Errors
/// Returns safe credential-resolution errors; credentials never enter the report.
pub(super) async fn prepare(
    api: &Api,
    context: &Result<credentials::Context, AgentSessionError>,
) -> Result<PreparedUsage<'static>, AgentSessionError> {
    let context = context.as_ref().map_err(Clone::clone)?;
    match context.resolve(api).await {
        Ok(Some(key)) => Ok(PreparedUsage {
            identity: Some(UsageIdentity::from_native(
                PROVIDER,
                key.expose_secret().as_bytes(),
            )),
            report: Box::pin(async move { api_key::read(&key).await }),
        }),
        Ok(None) => {
            let value = query_web(api).await;
            Ok(PreparedUsage {
                identity: None,
                report: Box::pin(async move { value }),
            })
        }
        Err(AgentSessionError::Unavailable) => Ok(PreparedUsage {
            identity: None,
            report: Box::pin(async {
                Ok(unavailable(
                    "This DSH DeepSeek credential source or endpoint does not support account balance inspection.",
                ))
            }),
        }),
        Err(error) => Err(error),
    }
}

async fn query_web(api: &Api) -> Result<Value, AgentSessionError> {
    let response = api
        .optional_call(
            "account/getBalance",
            json!({"client":{"version":env!("CARGO_PKG_VERSION"),
                "locale":"en","timezoneOffsetSeconds":0}}),
        )
        .await?;
    match response {
        Some(response) => project(&response),
        None => Ok(unavailable(
            "This DSH Host does not expose the DeepSeek account balance API.",
        )),
    }
}

/// Explain the explicit legacy transport's missing account API without implying a login failure.
pub(in crate::local::deepseek_harness) fn acp_unavailable() -> Value {
    unavailable("DSH ACP does not expose DeepSeek account balances. Use the native Host transport.")
}

fn unavailable(detail: &str) -> Value {
    let mut report = report(&[]);
    report["status"] = json!("unavailable");
    report["problem"] = json!({"kind":"no_quota","detail":detail});
    report
}

fn project(response: &Value) -> Result<Value, AgentSessionError> {
    if response.is_null() {
        return Ok(unavailable(
            "DSH did not return a signed-in DeepSeek account. Sign in to DeepSeek in DSH and refresh. API keys do not supply this account login.",
        ));
    }
    if response["status"] != "ready" {
        return Err(AgentSessionError::Failed);
    }
    let mut balances = Vec::with_capacity(4);
    for (field, kind, label) in [
        ("value", "recharge", "DeepSeek recharge balance"),
        ("bonusWallets", "bonus", "DeepSeek bonus balance"),
    ] {
        let wallets = response[field]
            .as_array()
            .filter(|wallets| wallets.len() <= 2)
            .ok_or(AgentSessionError::Failed)?;
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
            let remaining = amount(&wallet["balance"])?;
            balances.push(json!({"id":format!("{kind}-{unit}"),"label":label,
                "remaining":remaining,"unit":unit,"tone":"default"}));
        }
    }
    if balances.is_empty() {
        return Ok(unavailable(
            "DeepSeek did not report recharge or bonus balances for this account.",
        ));
    }
    Ok(report(&balances))
}

fn amount(value: &Value) -> Result<f64, AgentSessionError> {
    let text = value
        .as_str()
        .filter(|text| !text.is_empty() && text.len() <= 64 && text.trim() == *text)
        .ok_or(AgentSessionError::Failed)?;
    text.parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && value.abs() <= 9_007_199_254_740_991.0)
        .ok_or(AgentSessionError::Failed)
}

fn report(balances: &[Value]) -> Value {
    json!({"providerId":PROVIDER,"displayName":"DeepSeek Harness","status":"available",
        "planLabel":null,"windows":[],"balances":balances,"details":[],"error":null,
        "sourceLabel":"DSH DeepSeek account","fetchedAt":chrono::Utc::now().to_rfc3339(),
        "nextRefreshAt":null})
}

#[cfg(test)]
mod tests;
