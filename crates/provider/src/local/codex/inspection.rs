use chrono::{DateTime, Utc};
use serde_json::{Value, json};

use super::CodexClient;
use crate::ports::agent_session::{AgentClient, AgentSessionError};

impl CodexClient {
    pub(super) async fn native_diagnostic(&self) -> Result<String, AgentSessionError> {
        if !self.is_available().await? {
            return Ok("Codex executable: unavailable".to_owned());
        }
        let cwd = current_dir()?;
        let Ok(account) = self
            .query(&cwd, "account/read", json!({"refreshToken":false}))
            .await
        else {
            return Ok(
                "Codex executable: found\nNative account inspection: unavailable".to_owned(),
            );
        };
        let authentication = match account.pointer("/account/type").and_then(Value::as_str) {
            Some("chatgpt") => "ChatGPT login",
            Some("apiKey") => "API key",
            Some("amazonBedrock") => "Amazon Bedrock",
            _ => "not authenticated",
        };
        Ok(format!(
            "Codex executable: found\nApp-server protocol: available\nAuthentication: {authentication}"
        ))
    }

    pub(super) async fn native_usage(&self) -> Result<Value, AgentSessionError> {
        self.native_usage_in(&current_dir()?).await
    }

    /// Read account quota in the session directory using this client's launch environment.
    /// # Errors
    /// Returns native transport failure or malformed quota facts.
    pub(super) async fn native_usage_in(&self, cwd: &str) -> Result<Value, AgentSessionError> {
        self.prepare_native_usage_in(cwd).await?.report.await
    }

    /// Bind identity inspection and the lazy quota poll to one native app-server process.
    /// # Errors
    /// Returns native launch or initialization failures.
    pub(super) async fn prepare_native_usage_in<'a>(
        &'a self,
        cwd: &str,
    ) -> Result<crate::ports::account_usage::PreparedUsage<'a>, AgentSessionError> {
        use crate::ports::account_usage::{PreparedUsage, UsageIdentity};
        let mut transport = self.launch_transport(cwd, false)?;
        transport.initialize().await?;
        let account = transport
            .request("account/read", json!({"refreshToken":false}))
            .await
            .ok();
        let email = account
            .as_ref()
            .and_then(|account| account.pointer("/account/email"))
            .and_then(Value::as_str)
            .filter(|email| {
                !email.is_empty() && email.len() <= 256 && !email.chars().any(char::is_control)
            })
            .map(str::to_owned);
        let identity = email
            .as_ref()
            .and_then(|_| account.as_ref()?.get("account"))
            .and_then(|account| serde_json::to_vec(account).ok())
            .map(|account| UsageIdentity::from_native("codex", &account));
        Ok(PreparedUsage {
            identity,
            report: Box::pin(async move {
                let result = async {
                    let response = transport
                        .request("account/rateLimits/read", json!({}))
                        .await?;
                    let mut report = usage(&response)?;
                    if let Some(email) = email {
                        report["accountLabel"] = json!(email);
                    }
                    Ok(report)
                }
                .await;
                transport.close().await?;
                result
            }),
        })
    }

    /// Prepare host usage using the current launch directory and native credentials.
    /// # Errors
    /// Returns directory, native launch or initialization failures.
    pub(super) async fn prepare_native_usage(
        &self,
    ) -> Result<crate::ports::account_usage::PreparedUsage<'_>, AgentSessionError> {
        self.prepare_native_usage_in(&current_dir()?).await
    }
}

fn current_dir() -> Result<String, AgentSessionError> {
    std::env::current_dir()
        .map_err(|_| AgentSessionError::Failed)?
        .into_os_string()
        .into_string()
        .map_err(|_| AgentSessionError::Failed)
}

fn usage(response: &Value) -> Result<Value, AgentSessionError> {
    let mut windows = Vec::new();
    let mut plan = None;
    let snapshots: Vec<_> = if let Some(buckets) = response["rateLimitsByLimitId"]
        .as_object()
        .filter(|buckets| !buckets.is_empty())
    {
        buckets
            .iter()
            .map(|(id, snapshot)| (id.as_str(), snapshot))
            .collect()
    } else {
        vec![("codex", &response["rateLimits"])]
    };
    for (bucket, snapshot) in snapshots {
        if !snapshot.is_object() {
            return Err(AgentSessionError::Failed);
        }
        if plan.is_none() {
            plan = snapshot["planType"].as_str();
        }
        for (id, label) in [("primary", "Primary"), ("secondary", "Secondary")] {
            let window = &snapshot[id];
            if window.is_null() {
                continue;
            }
            let used = window["usedPercent"]
                .as_i64()
                .filter(|percent| *percent >= 0)
                .ok_or(AgentSessionError::Failed)?;
            let reset = window["resetsAt"]
                .as_i64()
                .map(|time| {
                    DateTime::from_timestamp(time, 0)
                        .ok_or(AgentSessionError::Failed)
                        .map(|time| time.to_rfc3339())
                })
                .transpose()?;
            let mut projected = json!({"id":format!("{bucket}:{id}"),"label":format!("{bucket} {label}"),"summary":bucket=="codex" && id=="primary","usedPct":used,"remainingPct":(100-used).max(0),
                "resetsAt":reset,"tone":if used>=95 {"danger"} else if used>=80 {"warning"} else {"ok"}});
            if let Some(minutes) = window["windowDurationMins"].as_u64() {
                projected["shortLabel"] = json!(if minutes >= 1440 {
                    format!("{}d", minutes / 1440)
                } else if minutes >= 60 {
                    format!("{}h", minutes / 60)
                } else {
                    format!("{minutes}m")
                });
            }
            windows.push(projected);
        }
    }
    Ok(
        json!({"providerId":"codex","displayName":"Codex","status":if windows.is_empty(){"unavailable"}else{"available"},"planLabel":plan,
        "sourceLabel":"Codex account/rateLimits/read","fetchedAt":Utc::now().to_rfc3339(),"nextRefreshAt":null,"windows":windows,"balances":[],"details":[],"error":null}),
    )
}

#[cfg(test)]
mod tests;
