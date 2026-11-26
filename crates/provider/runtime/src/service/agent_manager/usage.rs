//! Native account quotas stay in provider; no credentials or quota cache are persisted.

use provider_core::ports::account_usage::UsageIdentity;
use provider_core::ports::agent_session::AgentClient;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::Instant;

mod host;

use futures_util::{StreamExt, stream};
use model::ErrorCode;
use serde_json::{Value, json};

use super::AgentManager;
use crate::support::time::now_timestamp;
use provider_core::ports::agent_session::AgentSessionError;

/// Bounded ephemeral facts, always scoped to one Host or one live session generation.
#[derive(Debug, Clone)]
pub(super) struct UsageCache {
    entries: Arc<Mutex<BTreeMap<String, CachedUsage>>>,
    flights: Arc<Mutex<BTreeMap<String, host::Flight>>>,
    activity: Arc<Mutex<BTreeMap<String, Activity>>>,
    budget: Arc<tokio::sync::Semaphore>,
    tasks: tokio_util::task::TaskTracker,
    /// How long a snapshot answers reads that do not force a refresh.
    max_age: Duration,
}

impl Default for UsageCache {
    /// The cache of one live session.
    fn default() -> Self {
        Self::lasting(SESSION_MAX_AGE)
    }
}

/// Count of the quota-consuming events of one provider on this Host.
///
/// A snapshot read at an earlier count no longer describes the account.
#[derive(Debug, Clone, Default)]
pub(super) struct Activity(Arc<AtomicU64>);

impl Activity {
    /// Record that a call to the provider started, reported a request or ended.
    pub(super) fn mark(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }

    /// Events recorded so far.
    pub(super) fn count(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

const COLLECTION_TIMEOUT: Duration = Duration::from_secs(25);
/// Longest a snapshot answers reads once provider activity or a window reset followed it.
const OUTDATED_MAX_AGE: Duration = Duration::from_secs(300);
/// Longest a Host snapshot answers reads; it bounds what use on other devices can hide.
const HOST_MAX_AGE: Duration = Duration::from_mins(30);
const SESSION_MAX_AGE: Duration = Duration::from_secs(300);
/// Wait after the first failed read; each consecutive failure doubles it.
const ERROR_BACKOFF: Duration = Duration::from_secs(60);
const MAX_ERROR_BACKOFF: Duration = Duration::from_mins(15);

#[derive(Debug)]
struct CachedUsage {
    fetched: Instant,
    retry_until: Option<Instant>,
    /// First window reset that follows this read.
    reset_at: Option<Instant>,
    /// Consecutive failed reads ending with this one.
    failures: u32,
    /// Provider activity count this read could observe.
    activity: u64,
    source: Option<UsageIdentity>,
    identity: Option<UsageIdentity>,
    value: Value,
    successful: Option<Value>,
}

impl CachedUsage {
    fn retry_pending(&self) -> bool {
        self.retry_until
            .is_some_and(|retry_until| Instant::now() < retry_until)
    }

    /// Whether this snapshot answers a read instead of a new read of the account.
    ///
    /// An upstream retry time holds every read to it. Otherwise it answers reads that do not
    /// `force` a refresh for `max_age`, or less when it is a failure or when the provider's
    /// `activity` count or a window reset has moved past it.
    fn answers(&self, force: bool, max_age: Duration, activity: u64) -> bool {
        if self.retry_pending() {
            return true;
        }
        let lifetime = if self.value["status"] == "error" {
            error_backoff(self.failures)
        } else if self.activity != activity
            || self.reset_at.is_some_and(|reset| Instant::now() >= reset)
        {
            OUTDATED_MAX_AGE
        } else {
            max_age
        };
        !force && self.fetched.elapsed() < lifetime.min(max_age)
    }
}

/// How long the `failures`-th consecutive failed read answers reads that do not force a refresh.
fn error_backoff(failures: u32) -> Duration {
    ERROR_BACKOFF
        .saturating_mul(1 << failures.saturating_sub(1).min(4))
        .min(MAX_ERROR_BACKOFF)
}

/// Convert an RFC 3339 time once so wall-clock changes cannot move the deadline.
///
/// Returns nothing for a missing, malformed or past time.
fn deadline(at: &Value) -> Option<Instant> {
    let at = chrono::DateTime::parse_from_rfc3339(at.as_str()?).ok()?;
    let delay = at.signed_duration_since(chrono::Utc::now()).to_std().ok()?;
    Instant::now().checked_add(delay)
}

/// Earliest time an error allows the next read, at most one hour away.
fn retry_deadline(value: &Value) -> Option<Instant> {
    if value["status"] != "error" {
        return None;
    }
    let limit = Instant::now() + Duration::from_secs(3600);
    deadline(&value["nextRefreshAt"]).map(|retry| retry.min(limit))
}

/// First reset among the windows of `value` that is still ahead.
fn reset_deadline(value: &Value) -> Option<Instant> {
    value["windows"]
        .as_array()?
        .iter()
        .filter_map(|window| deadline(&window["resetsAt"]))
        .min()
}

impl UsageCache {
    /// The Host's shared cache, which outlives sessions and follows provider activity.
    pub(super) fn for_host() -> Self {
        Self::lasting(HOST_MAX_AGE)
    }

    fn lasting(max_age: Duration) -> Self {
        Self {
            entries: Arc::default(),
            flights: Arc::default(),
            activity: Arc::default(),
            budget: Arc::new(tokio::sync::Semaphore::new(4)),
            tasks: tokio_util::task::TaskTracker::new(),
            max_age,
        }
    }

    /// The activity counter of `provider`, shared by every session that runs it.
    pub(super) fn activity(&self, provider: &str) -> Activity {
        self.activity.lock().map_or_else(
            |_| Activity::default(),
            |mut activity| activity.entry(provider.to_owned()).or_default().clone(),
        )
    }

    fn activity_count(&self, provider: &str) -> u64 {
        self.activity
            .lock()
            .ok()
            .and_then(|activity| activity.get(provider).map(Activity::count))
            .unwrap_or_default()
    }

    fn get(&self, provider: &str, identity: &UsageIdentity, force: bool) -> Option<Value> {
        let activity = self.activity_count(provider);
        self.entries
            .lock()
            .ok()?
            .get(provider)
            .filter(|cached| {
                cached.identity.as_ref() == Some(identity)
                    && cached.answers(force, self.max_age, activity)
            })
            .map(|cached| cached.value.clone())
    }

    fn put(
        &self,
        provider: &str,
        identity: Option<UsageIdentity>,
        mut value: Value,
        attempted_at: &str,
    ) -> Value {
        value["fetchedAt"] = json!(attempted_at);
        let activity = self.activity_count(provider);
        let Ok(mut cache) = self.entries.lock() else {
            return value;
        };
        let previous = cache
            .remove(provider)
            .filter(|cached| identity.is_some() && cached.identity == identity);
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
            let failures = if value["status"] == "error" {
                previous.as_ref().map_or(0, |cached| cached.failures) + 1
            } else {
                0
            };
            cache.insert(
                provider.to_owned(),
                CachedUsage {
                    fetched: Instant::now(),
                    retry_until: retry_deadline(&value),
                    reset_at: reset_deadline(&value),
                    failures,
                    activity,
                    source: None,
                    identity: Some(identity),
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
        prepared: Result<provider_core::ports::account_usage::PreparedUsage<'_>, AgentSessionError>,
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
    /// Drain shared quota work and reap provider query processes before the worker exits.
    /// # Errors
    /// Returns the first native close failure after attempting every client.
    pub(crate) async fn close_host_usage(&self) -> Result<(), AgentSessionError> {
        self.usage_cache.close().await?;
        let mut failure = None;
        for client in self.clients.values() {
            if let Err(error) = client.close_usage().await {
                failure.get_or_insert(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }

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
        let started = Instant::now();
        let mut providers = if let Some(id) = agent_id {
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
                tokio::time::timeout(COLLECTION_TIMEOUT, async {
                    agent
                        .usage_cache
                        .read(
                            &record.provider,
                            agent.session.prepare_account_usage().await,
                            force,
                        )
                        .await
                })
                .await
                .unwrap_or_else(|_| failure(&record.provider, AgentSessionError::Failed))
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
                    .map(|(_, client)| async move {
                        self.usage_cache.host(client.clone(), force, started).await
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
        providers.retain(|provider| provider["status"] != "unavailable");
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
