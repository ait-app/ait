//! Local checkout status reads after Agent turns end, at most once a minute per checkout.
//!
//! An Agent that finishes or fails a turn has usually just edited or committed in its checkout,
//! which changes whether clients compare uncommitted changes or the branch against its base.
//! Only the local status is read; remote refs keep their own fetch interval.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use domain::agent_runtime::registry::AgentRuntimeRegistry;
use domain::session::protocol::SessionEventKind;
use serde_json::Value;
use tokio::sync::broadcast;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use super::{StatusPublisher, status_snapshot};
use crate::git::ports::git_fetch::GitFetchRuntime;

/// Shortest time between two status reads of the same checkout.
const MIN_INTERVAL: Duration = Duration::from_secs(60);

/// Per-checkout schedule that reads at once, then at most once per [`MIN_INTERVAL`]; requests
/// inside the interval collapse into one read when it elapses.
#[derive(Debug, Default)]
struct Throttle {
    /// When each checkout was last read, kept only while it still delays the next read.
    last_read: BTreeMap<String, Instant>,
    /// When each requested checkout becomes due.
    due: BTreeMap<String, Instant>,
}

impl Throttle {
    /// Request a read of `cwd`, due now unless it was read within the last interval.
    fn request(&mut self, cwd: String, now: Instant) {
        let due = self
            .last_read
            .get(&cwd)
            .map_or(now, |last| (*last + MIN_INTERVAL).max(now));
        self.due.entry(cwd).or_insert(due);
    }

    /// The earliest pending deadline, or None when nothing is requested.
    fn next_due(&self) -> Option<Instant> {
        self.due.values().min().copied()
    }

    /// Remove and return the checkouts due by `now`, recording them as read at `now`.
    fn take_due(&mut self, now: Instant) -> Vec<String> {
        self.last_read.retain(|_, last| *last + MIN_INTERVAL > now);
        let ready: Vec<String> = self
            .due
            .iter()
            .filter(|(_, due)| **due <= now)
            .map(|(cwd, _)| cwd.clone())
            .collect();
        for cwd in &ready {
            self.due.remove(cwd);
            self.last_read.insert(cwd.clone(), now);
        }
        ready
    }
}

/// Publish the local status of each Agent's checkout after its turn ends, until `cancellation`.
///
/// # Arguments
///
/// * `attention` - Committed attention events, subscribed before the task was admitted
/// * `agents` - Registry resolving an Agent identity to its checkout directory
/// * `backend` - Blocking Git adapter used for the status read
/// * `publisher` - Status publisher shared with the background fetch workers
/// * `cancellation` - Server cancellation ending the task
pub(super) async fn refresh_after_turns(
    mut attention: broadcast::Receiver<(SessionEventKind, Value)>,
    agents: Arc<dyn AgentRuntimeRegistry>,
    backend: Arc<dyn GitFetchRuntime>,
    publisher: StatusPublisher,
    cancellation: CancellationToken,
) {
    let mut throttle = Throttle::default();
    loop {
        let next_due = throttle.next_due();
        tokio::select! {
            biased;
            () = cancellation.cancelled() => break,
            () = sleep_until(next_due) => {}
            event = attention.recv() => match event {
                Ok((kind, payload)) => {
                    if let Some(cwd) = turn_end_checkout(&agents, kind, &payload).await {
                        throttle.request(cwd, Instant::now());
                    }
                }
                // Lost turn ends are covered by the next client refresh or remote fetch.
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            },
        }
        let ready = throttle.take_due(Instant::now());
        if !ready.is_empty() {
            publish_statuses(ready, &backend, &publisher, &cancellation).await;
        }
    }
}

/// Sleep until `deadline`, or forever without one.
async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

/// The checkout directory of the Agent whose turn finished or failed in this event.
async fn turn_end_checkout(
    agents: &Arc<dyn AgentRuntimeRegistry>,
    kind: SessionEventKind,
    payload: &Value,
) -> Option<String> {
    if kind != SessionEventKind::AgentAttention
        || !matches!(payload["reason"].as_str(), Some("finished" | "error"))
    {
        return None;
    }
    let agent_id = payload["agentId"].as_str()?.to_owned();
    let registry = agents.clone();
    match tokio::task::spawn_blocking(move || registry.get(&agent_id)).await {
        Ok(Ok(record)) => record.map(|record| record.cwd),
        Ok(Err(error)) => {
            tracing::warn!(%error, "cannot read the Agent whose turn ended");
            None
        }
        Err(_) => None,
    }
}

/// Read and publish the status of each checkout in `paths` outside the reactor.
async fn publish_statuses(
    paths: Vec<String>,
    backend: &Arc<dyn GitFetchRuntime>,
    publisher: &StatusPublisher,
    cancellation: &CancellationToken,
) {
    let backend = backend.clone();
    let publisher = publisher.clone();
    let cancellation = cancellation.clone();
    let _ = tokio::task::spawn_blocking(move || {
        for path in paths {
            if cancellation.is_cancelled() {
                break;
            }
            if let Some(snapshot) = status_snapshot(backend.as_ref(), &path) {
                publisher.publish(path, snapshot);
            }
        }
    })
    .await;
}

#[cfg(test)]
mod tests;
