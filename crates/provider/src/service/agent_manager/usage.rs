//! Native account quotas stay in provider; no credentials or quota cache are persisted.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::{StreamExt, stream};
use model::ErrorCode;
use serde_json::{Value, json};

use super::{AgentManager, now_timestamp};
use crate::ports::agent_session::AgentSessionError;

/// Bounded, ephemeral host quota snapshots shared by independent execution lanes.
#[derive(Debug, Clone, Default)]
pub(super) struct UsageCache(Arc<Mutex<BTreeMap<String, (Instant, Value)>>>);

impl UsageCache {
    fn get(&self, provider: &str, force: bool) -> Option<Value> {
        if force {
            return None;
        }
        self.0
            .lock()
            .ok()?
            .get(provider)
            .filter(|(fetched, _)| fetched.elapsed() < Duration::from_secs(300))
            .map(|(_, value)| value.clone())
    }

    fn put(&self, provider: &str, value: &Value) {
        if let Ok(mut cache) = self.0.lock() {
            if cache.len() >= 64 {
                cache.clear();
            }
            cache.insert(provider.to_owned(), (Instant::now(), value.clone()));
        }
    }
}

impl AgentManager {
    /// Read a live session's account or host Provider quotas, optionally refreshing one source.
    /// # Errors
    /// Returns unknown identity, invalid scope, registry failure or an oversized projection.
    pub(crate) async fn usage(
        &self,
        agent_id: Option<&str>,
        provider_id: Option<&str>,
        force: bool,
    ) -> Result<Value, ErrorCode> {
        if provider_id.is_some_and(|id| !self.clients.contains_key(id)) {
            return Err(ErrorCode::UnsupportedCapability);
        }
        let providers = if let Some(id) = agent_id {
            let record = self
                .registry
                .get(id)
                .map_err(|_| ErrorCode::AgentIo)?
                .ok_or(ErrorCode::AgentNotFound)?;
            if provider_id.is_some_and(|id| id != record.provider) {
                return Err(ErrorCode::InvalidMessage);
            }
            // A stopped/imported session has no current launch account. Never substitute the
            // daemon's login for a session created under an ephemeral environment.
            let value = if let Some(agent) = self.live.get(id) {
                agent
                    .session
                    .account_usage()
                    .await
                    .unwrap_or_else(|error| failure(&record.provider, error))
            } else {
                unavailable(
                    &record.provider,
                    "Start or resume this agent to read its current account usage",
                )
            };
            vec![value]
        } else {
            let mut providers: Vec<_> = stream::iter(
                self.clients
                    .iter()
                    .filter(|(id, _)| provider_id.is_none_or(|provider| provider == id.as_str()))
                    .map(|(id, client)| async move {
                        if let Some(value) = self.usage_cache.get(id, force) {
                            return value;
                        }
                        let value = client
                            .usage()
                            .await
                            .unwrap_or_else(|error| failure(id, error));
                        self.usage_cache.put(id, &value);
                        value
                    }),
            )
            .buffer_unordered(4)
            .collect()
            .await;
            providers.sort_by(|left, right| {
                left["providerId"]
                    .as_str()
                    .cmp(&right["providerId"].as_str())
            });
            providers
        };
        crate::rpc::timeline::bounded(json!({"fetchedAt":now_timestamp(),"providers":providers}))
    }
}

fn unavailable(provider: &str, detail: &str) -> Value {
    json!({"providerId":provider,"displayName":provider,"status":"unavailable",
        "planLabel":null,"windows":[],"problem":{"kind":"no_quota","detail":detail}})
}

fn failure(provider: &str, error: AgentSessionError) -> Value {
    if error == AgentSessionError::Unavailable {
        unavailable(
            provider,
            "This login does not report plan usage. Sign in with the provider CLI.",
        )
    } else {
        json!({"providerId":provider,"displayName":provider,"status":"error",
            "planLabel":null,"windows":[],"error":"Unable to read native account usage"})
    }
}

#[cfg(test)]
mod tests;
