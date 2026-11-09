//! Mechanics shared by status-driven channel onboarding flows.
//!
//! Clients drive progress with status requests, so an abandoned flow needs no background work.
//! Each Workspace has at most one flow per platform, held in memory until a daemon restart.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use domain::ChannelId;
use domain::bot::ChannelConfig;
use model::ErrorCode;
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::sync::{Mutex as SessionLock, OwnedMutexGuard};
use tokio::time::Instant;
use uuid::Uuid;

use crate::protocol::ScanRequest;
use crate::service::Bots;
use crate::state::StoredChannel;

pub(super) const FAILED_EXPIRED: &str = "expired";
pub(super) const FAILED_UNAVAILABLE: &str = "unavailable";
pub(super) const FAILED_ALREADY_CONNECTED: &str = "alreadyConnected";

pub(super) type Session<T> = Arc<SessionLock<T>>;

/// The current onboarding flow of each Workspace.
#[derive(Debug)]
pub(super) struct Sessions<T>(Mutex<BTreeMap<String, Session<T>>>);

impl<T> Default for Sessions<T> {
    fn default() -> Self {
        Self(Mutex::default())
    }
}

impl<T> Sessions<T> {
    fn entries(&self) -> MutexGuard<'_, BTreeMap<String, Session<T>>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn get(&self, workspace: &str) -> Option<Session<T>> {
        self.entries().get(workspace).cloned()
    }

    /// Install `session`, replacing the Workspace's earlier flow.
    pub(super) fn insert(&self, workspace: String, session: T) {
        self.entries()
            .insert(workspace, Arc::new(SessionLock::new(session)));
    }

    /// Forget `session` unless a newer flow has already replaced it.
    pub(super) fn remove(&self, workspace: &str, session: &Session<T>) {
        let mut entries = self.entries();
        if entries
            .get(workspace)
            .is_some_and(|current| Arc::ptr_eq(current, session))
        {
            entries.remove(workspace);
        }
    }
}

/// A QR scan driven by status requests: `F` is the platform's handle on the scan and `A` the
/// credentials it returned, kept until the channel is saved because they are issued only once.
#[derive(Debug)]
pub(super) struct Scan<F, A> {
    pub(super) id: Uuid,
    pub(super) channel_id: ChannelId,
    pub(super) client: reqwest::Client,
    pub(super) allow_groups: bool,
    pub(super) flow: F,
    pub(super) approved: Option<A>,
    interval: Duration,
    pub(super) next_poll: Instant,
    deadline: Instant,
    outcome: Option<Value>,
}

impl<F, A> Scan<F, A> {
    pub(super) fn new(
        client: reqwest::Client,
        flow: F,
        interval: Duration,
        lifetime_seconds: u64,
        allow_groups: bool,
    ) -> Self {
        let now = Instant::now();
        Self {
            id: Uuid::new_v4(),
            channel_id: ChannelId::generate(),
            client,
            allow_groups,
            flow,
            approved: None,
            interval,
            next_poll: now,
            deadline: now + Duration::from_secs(lifetime_seconds),
            outcome: None,
        }
    }

    pub(super) fn finish(&mut self, outcome: Value) -> Value {
        self.outcome = Some(outcome.clone());
        outcome
    }

    /// The response to give without asking the platform, or `None` when a poll is due.
    pub(super) fn before_poll(&mut self) -> Option<Value> {
        if let Some(outcome) = &self.outcome {
            return Some(outcome.clone());
        }
        let now = Instant::now();
        // Issued credentials cannot be requested again, so a pending save outlives the code.
        if now >= self.deadline && self.approved.is_none() {
            return Some(self.finish(failed(FAILED_EXPIRED)));
        }
        if now < self.next_poll {
            return Some(waiting());
        }
        self.next_poll = now + self.interval;
        None
    }

    /// Save the approved channel, keeping the credentials to retry after a transient failure.
    pub(super) async fn save_approved(
        &mut self,
        bots: &Bots,
        workspace_id: String,
        config: ChannelConfig,
        secret: SecretString,
    ) -> Value {
        let channel = StoredChannel {
            workspace_id,
            channel_id: self.channel_id,
            config,
        };
        match save_channel(bots, channel, secret).await {
            Some(outcome) => {
                self.approved = None;
                self.finish(outcome)
            }
            None => waiting(),
        }
    }
}

impl<F, A> Sessions<Scan<F, A>> {
    /// Lock the scan a status request names, if it is still the Workspace's current scan.
    pub(super) async fn current(
        &self,
        request: &ScanRequest,
    ) -> Option<OwnedMutexGuard<Scan<F, A>>> {
        let scan = self.get(&request.workspace_id)?.lock_owned().await;
        (scan.id == request.scan_id).then_some(scan)
    }

    /// Stop a scan that has not finished.
    pub(super) async fn cancel(&self, request: &ScanRequest) -> Value {
        let Some(entry) = self.get(&request.workspace_id) else {
            return json!({"cancelled":false});
        };
        {
            let mut scan = entry.lock().await;
            if scan.id != request.scan_id || scan.outcome.is_some() {
                return json!({"cancelled":false});
            }
            scan.approved = None;
            scan.finish(failed(FAILED_EXPIRED));
        }
        self.remove(&request.workspace_id, &entry);
        json!({"cancelled":true})
    }
}

pub(super) fn waiting() -> Value {
    json!({"state":"waiting"})
}

pub(super) fn failed(reason: &str) -> Value {
    json!({"state":"failed","reason":reason})
}

pub(super) fn http_client() -> Result<reqwest::Client, ErrorCode> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| ErrorCode::BotIo)
}

/// Save a verified channel and return the flow's final outcome, or `None` to retry on the next
/// status after a transient failure.
pub(super) async fn save_channel(
    bots: &Bots,
    channel: StoredChannel,
    secret: SecretString,
) -> Option<Value> {
    match bots.create_stored_channel(channel, secret).await {
        Ok(created) => Some(json!({"state":"complete","channel":created["channel"]})),
        Err(error) if error.retryable() => {
            tracing::warn!(?error, "will retry saving an onboarded channel");
            None
        }
        Err(error) => {
            tracing::warn!(?error, "could not save an onboarded channel");
            Some(failed(FAILED_UNAVAILABLE))
        }
    }
}

#[cfg(test)]
mod tests;
