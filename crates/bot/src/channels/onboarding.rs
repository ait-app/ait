//! Mechanics shared by status-driven channel onboarding flows.
//!
//! Clients drive progress with status requests, so an abandoned flow needs no background work.
//! Each Workspace has at most one flow per platform, held in memory until a daemon restart.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use model::ErrorCode;
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::sync::Mutex as SessionLock;

use crate::service::Bots;
use crate::state::StoredChannel;

pub(super) const FAILED_EXPIRED: &str = "expired";
pub(super) const FAILED_UNAVAILABLE: &str = "unavailable";

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
