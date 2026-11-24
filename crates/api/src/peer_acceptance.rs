//! Whether this Host accepts calls from Agents on sibling Hosts of the account, for the daemon's
//! publication to the account center.
//!
//! The value is `mcp.enabled && mcp.acceptPeers`. It is read once at startup and then follows
//! every committed daemon configuration change through the same `status.daemon_config_changed`
//! event clients observe, so a `daemon.config.set.request` takes effect without a restart.

use std::sync::Arc;

use domain::session::SessionError;
use domain::session::protocol::{EventsRequest, SessionEventKind};
use model::session::{SessionConnection, SessionEvents, SessionSubscription};
use serde_json::Value;
use tokio::sync::watch;

/// The current acceptance and the observer that keeps it current.
#[derive(Debug)]
pub(crate) struct PeerAcceptance {
    accepting: watch::Receiver<bool>,
    /// Registered with the session events for the API's lifetime; dropping it ends observation.
    _observer: (SessionConnection, SessionSubscription),
}

impl PeerAcceptance {
    /// Read the daemon configuration once and follow its committed changes on `metadata`.
    ///
    /// # Errors
    /// Returns `ServiceInitialization` if the observer cannot be registered.
    pub(crate) fn start(metadata: &metadata::dispatch::State) -> Result<Self, crate::ConfigError> {
        let daemon = metadata.daemon.as_deref();
        Self::follow(&metadata.session_events, || read(daemon))
            .map_err(|_| crate::ConfigError::ServiceInitialization)
    }

    /// Follow committed configuration events published on `events`, starting from `initial`.
    ///
    /// `initial` is read only after the observer is registered, and while the watch is locked
    /// against the observer, so a commit either is part of that read or arrives as a later event.
    ///
    /// # Errors
    /// Returns the session error if the observer cannot be registered.
    fn follow(
        events: &SessionEvents,
        initial: impl FnOnce() -> bool,
    ) -> Result<Self, SessionError> {
        let (sender, receiver) = watch::channel(false);
        let observer = sender.clone();
        let connection = events.connect();
        let subscription = connection.subscribe(
            EventsRequest {
                events: vec![SessionEventKind::DaemonConfig.method().to_owned()],
                notifications: false,
            },
            Arc::new(move |_, payload: Value| {
                let next = accepting(&payload["config"]);
                observer.send_if_modified(|current| std::mem::replace(current, next) != next);
                Ok(())
            }),
        )?;
        subscription.activate()?;
        sender.send_if_modified(|current| {
            let next = initial();
            std::mem::replace(current, next) != next
        });
        Ok(Self {
            accepting: receiver,
            _observer: (connection, subscription),
        })
    }

    /// Return a new observer of the current acceptance.
    pub(crate) fn receiver(&self) -> watch::Receiver<bool> {
        self.accepting.clone()
    }
}

/// Read the acceptance once at startup. Without the daemon service, or when its configuration
/// cannot be read, this Host does not accept.
fn read(daemon: Option<&metadata::daemon::service::Daemon>) -> bool {
    daemon.is_some_and(|daemon| match daemon.get_config() {
        Ok(config) => accepting(&config),
        Err(error) => {
            tracing::warn!(%error, "Daemon configuration unreadable; not accepting peer calls");
            false
        }
    })
}

/// Whether `config` lets sibling Hosts call this Host. An absent `mcp.enabled` means enabled,
/// as on the MCP route; an absent `mcp.acceptPeers` means off.
pub(crate) fn accepting(config: &Value) -> bool {
    config["mcp"]["enabled"] != false && config["mcp"]["acceptPeers"] == true
}

#[cfg(test)]
mod tests;
