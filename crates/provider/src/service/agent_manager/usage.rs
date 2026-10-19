//! Native account quotas stay in provider; no credentials or quota cache are persisted.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::{StreamExt, stream};
use model::ErrorCode;
use serde_json::{Value, json};

use super::AgentManager;
use crate::ports::agent_session::AgentSessionError;
use crate::support::time::now_timestamp;

/// Bounded ephemeral facts, always scoped to one Host or one live session generation.
#[derive(Debug, Clone, Default)]
pub(super) struct UsageCache(Arc<Mutex<BTreeMap<String, CachedUsage>>>);

#[derive(Debug)]
struct CachedUsage {
    fetched: Instant,
    identity: crate::ports::account_usage::UsageIdentity,
    value: Value,
    successful: Option<Value>,
}

impl UsageCache {
    fn get(
        &self,
        provider: &str,
        identity: &crate::ports::account_usage::UsageIdentity,
        force: bool,
    ) -> Option<Value> {
        if force {
            return None;
        }
        self.0
            .lock()
            .ok()?
            .get(provider)
            .filter(|cached| {
                &cached.identity == identity && cached.fetched.elapsed() < Duration::from_secs(300)
            })
            .map(|cached| cached.value.clone())
    }

    fn put(
        &self,
        provider: &str,
        identity: Option<crate::ports::account_usage::UsageIdentity>,
        mut value: Value,
        attempted_at: &str,
    ) -> Value {
        value["fetchedAt"] = json!(attempted_at);
        let Ok(mut cache) = self.0.lock() else {
            return value;
        };
        let previous = cache
            .remove(provider)
            .filter(|cached| Some(&cached.identity) == identity.as_ref());
        let available = value["status"] == "available";
        let successful = if available {
            let successful_at = now_timestamp();
            value["successfulAt"] = json!(successful_at);
            let mut report = json!({"status":"available","windows":value["windows"]});
            for key in ["planLabel", "balances", "details"] {
                if !value[key].is_null() {
                    report[key] = value[key].clone();
                }
            }
            Some(json!({"fetchedAt":successful_at,"report":report}))
        } else {
            previous
                .as_ref()
                .and_then(|cached| cached.successful.clone())
        };
        if !available && let Some(snapshot) = &successful {
            value["lastSuccessful"] = snapshot.clone();
            if value["accountLabel"].is_null()
                && let Some(previous) = &previous
                && let Some(label) = previous.value["accountLabel"].as_str()
            {
                value["accountLabel"] = json!(label);
            }
        }
        if let Some(identity) = identity {
            if cache.len() >= 64 {
                cache.pop_first();
            }
            cache.insert(
                provider.to_owned(),
                CachedUsage {
                    fetched: Instant::now(),
                    identity,
                    value: value.clone(),
                    successful,
                },
            );
        }
        value
    }

    async fn read(
        &self,
        provider: &str,
        prepared: Result<crate::ports::account_usage::PreparedUsage<'_>, AgentSessionError>,
        force: bool,
    ) -> Value {
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                return self.put(provider, None, failure(provider, error), &now_timestamp());
            }
        };
        if let Some(identity) = &prepared.identity
            && let Some(value) = self.get(provider, identity, force)
        {
            return value;
        }
        let attempted_at = now_timestamp();
        let value = prepared
            .report
            .await
            .unwrap_or_else(|error| failure(provider, error));
        self.put(provider, prepared.identity, value, &attempted_at)
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
                    .usage_cache
                    .read(
                        &record.provider,
                        agent.session.prepare_account_usage().await,
                        force,
                    )
                    .await
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
                        self.usage_cache
                            .read(id, client.prepare_usage().await, force)
                            .await
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
