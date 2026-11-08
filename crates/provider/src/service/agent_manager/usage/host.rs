//! Global host snapshots: serve cached facts while one bounded background refresh runs.
use super::{
    AgentClient, AgentSessionError, COLLECTION_TIMEOUT, Duration, Instant, MAX_AGE, REFRESH_AFTER,
    UsageCache, UsageIdentity, Value, failure, now_timestamp,
};
use std::sync::Arc;
use tokio::sync::watch;
use uuid::Uuid;

#[derive(Debug)]
pub(super) struct Flight {
    generation: Uuid,
    source: Option<UsageIdentity>,
    result: watch::Receiver<Option<Value>>,
}

impl UsageCache {
    /// Read a host snapshot or join a refresh; cheap credential checks precede cache reuse.
    pub(super) async fn host(
        &self,
        client: Arc<dyn AgentClient>,
        force: bool,
        started: Instant,
    ) -> Value {
        let provider = client.provider().to_owned();
        let deadline = started + COLLECTION_TIMEOUT;
        let Ok(source) = tokio::time::timeout_at(deadline, client.usage_cache_key()).await else {
            return failure(&provider, AgentSessionError::Failed);
        };
        if !force && let Some((cached, age)) = self.snapshot(&provider, source.as_ref()) {
            if age >= REFRESH_AFTER {
                // Own the refresh independently of the RPC/UI connection lifetime.
                let _ = self.refresh_host(client, source, deadline, true);
            }
            return cached;
        }
        let mut result = match self.refresh_host(client, source, deadline, force) {
            Ok(result) => result,
            Err(error) => return failure(&provider, error),
        };
        match result.wait_for(Option::is_some).await {
            Ok(value) => value
                .clone()
                .unwrap_or_else(|| failure(&provider, AgentSessionError::Failed)),
            Err(_) => failure(&provider, AgentSessionError::Failed),
        }
    }

    fn snapshot(
        &self,
        provider: &str,
        source: Option<&UsageIdentity>,
    ) -> Option<(Value, Duration)> {
        let source = source?;
        let entries = self.entries.lock().ok()?;
        let entry = entries.get(provider)?;
        let age = entry.fetched.elapsed();
        let ttl = if entry.value["status"] == "error" {
            Duration::from_secs(15)
        } else {
            MAX_AGE
        };
        (entry.source.as_ref() == Some(source) && age < ttl).then(|| (entry.value.clone(), age))
    }

    fn refresh_host(
        &self,
        client: Arc<dyn AgentClient>,
        source: Option<UsageIdentity>,
        deadline: Instant,
        force: bool,
    ) -> Result<watch::Receiver<Option<Value>>, AgentSessionError> {
        let provider = client.provider().to_owned();
        let mut flights = self.flights.lock().map_err(|_| AgentSessionError::Failed)?;
        if let Some(flight) = flights.get(&provider)
            && flight.source == source
            && flight.result.borrow().is_none()
            && flight.result.has_changed().is_ok()
        {
            return Ok(flight.result.clone());
        }
        let generation = Uuid::new_v4();
        let (sender, result) = watch::channel(None);
        flights.insert(
            provider.clone(),
            Flight {
                generation,
                source: source.clone(),
                result: result.clone(),
            },
        );
        let cache = self.clone();
        tokio::spawn(async move {
            let attempted = now_timestamp();
            let mut known_identity = cache.entries.lock().ok().and_then(|entries| {
                entries
                    .get(&provider)
                    .filter(|entry| source.is_some() && entry.source == source)
                    .map(|entry| entry.identity.clone())
            });
            let poll = async {
                let _permit = cache
                    .budget
                    .acquire()
                    .await
                    .map_err(|_| AgentSessionError::Failed)?;
                let prepared = client.prepare_usage().await?;
                // A changed local credential/config generation must collect again even
                // if native account labels match (for example another workspace).
                if !force
                    && source.is_none()
                    && let Some(identity) = &prepared.identity
                    && let Some(value) = cache.get(&provider, identity, false)
                {
                    if client.usage_cache_key().await != source {
                        known_identity = None;
                        return Err(AgentSessionError::Failed);
                    }
                    return Ok((prepared.identity, value, false));
                }
                let identity = prepared.identity;
                known_identity.clone_from(&identity);
                let value = prepared
                    .report
                    .await
                    .unwrap_or_else(|error| failure(&provider, error));
                if client.usage_cache_key().await != source {
                    known_identity = None;
                    return Err(AgentSessionError::Failed);
                }
                Ok((identity, value, true))
            };
            let polled = tokio::time::timeout_at(deadline, poll).await;
            let (identity, value, changed) = match polled {
                Ok(Ok(report)) => report,
                _ => (
                    known_identity,
                    failure(&provider, AgentSessionError::Failed),
                    true,
                ),
            };
            let value = if let Ok(flights) = cache.flights.lock() {
                if flights
                    .get(&provider)
                    .is_some_and(|flight| flight.generation == generation)
                {
                    let value = if changed {
                        // Native labels may match across workspaces; history also belongs
                        // to the verified local credential/config generation.
                        if let Ok(mut entries) = cache.entries.lock()
                            && entries
                                .get(&provider)
                                .is_some_and(|entry| entry.source != source)
                        {
                            entries.remove(&provider);
                        }
                        cache.put(&provider, identity, value, &attempted)
                    } else {
                        value
                    };
                    if let Ok(mut entries) = cache.entries.lock()
                        && let Some(entry) = entries.get_mut(&provider)
                    {
                        entry.source = source;
                    }
                    value
                } else {
                    failure(&provider, AgentSessionError::Failed)
                }
            } else {
                failure(&provider, AgentSessionError::Failed)
            };
            sender.send_replace(Some(value));
        });
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
