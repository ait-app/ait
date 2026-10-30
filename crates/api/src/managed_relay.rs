//! Internal capability for a center-managed daemon; never exposed through RPC.
use std::sync::{Arc, Mutex};

use host_link::{Binding, CENTER, Error, ManagedRelay};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use uuid::Uuid;

/// Internal exclusive Relay capability issued before the API is shared.
#[derive(Debug, Clone)]
pub struct ManagedRelayHandle {
    pub(super) relay: relay::Connector,
    pub(super) hub: relay::hub::Connector,
    pub(super) status: Arc<Mutex<Value>>,
}

impl ManagedRelay for ManagedRelayHandle {
    async fn start(&self, session: Uuid, ticket: SecretString) -> Result<(), Error> {
        self.relay
            .start(relay::ControlGrant {
                center_url: CENTER.to_owned(),
                control_ticket: ticket.expose_secret().to_owned(),
                node_session_id: session,
            })
            .await
            .map_err(|_| Error::Unavailable)
    }

    async fn active(&self) -> bool {
        let status = self.relay.status().await;
        status.online || status.connecting
    }

    async fn online(&self) -> bool {
        self.relay.status().await.online
    }

    async fn hub_online(&self) -> bool {
        self.hub.status().await.online
    }

    async fn stop(&self) {
        self.relay.stop().await;
        self.hub.stop().await;
    }

    async fn start_hub(&self, session: Uuid, ticket: SecretString) -> Result<(), Error> {
        self.hub
            .start(relay::hub::Grant {
                center_url: CENTER.to_owned(),
                hub_ticket: ticket,
                node_session_id: session,
            })
            .await
            .map_err(|_| Error::Unavailable)
    }

    async fn hub_active(&self) -> bool {
        let status = self.hub.status().await;
        // A link another process took over is left to it for a while.
        status.online || status.connecting || self.hub.yielding().await
    }

    fn report(&self, phase: &'static str, binding: Option<&Binding>) {
        if let Ok(mut status) = self.status.lock() {
            *status = json!({"mode":"managed", "phase":phase, "binding":binding});
        }
    }
}
