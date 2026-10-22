//! Internal capability for a center-managed daemon; never exposed through RPC.
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use host_link::{Binding, CENTER, Error, ManagedHub};
use secrecy::SecretString;
use serde_json::{Value, json};
use uuid::Uuid;

/// Internal exclusive Hub capability issued before the API is shared.
#[derive(Debug)]
pub struct ManagedHubHandle {
    pub(super) hub: hub::Connector,
    pub(super) status: Arc<Mutex<Value>>,
}

#[async_trait]
impl ManagedHub for ManagedHubHandle {
    async fn start(&self, session: Uuid, ticket: SecretString) -> Result<(), Error> {
        self.hub
            .start(hub::ControlGrant {
                center_url: CENTER.to_owned(),
                hub_ticket: ticket,
                node_session_id: session,
            })
            .await
            .map_err(|_| Error::Unavailable)
    }

    async fn active(&self) -> bool {
        let status = self.hub.status().await;
        status.online
            || status.connecting
            || matches!(status.error, Some("replaced" | "unauthorized"))
    }

    async fn stop(&self) {
        self.hub.stop().await;
    }

    fn report(&self, phase: &'static str, binding: Option<&Binding>) {
        if let Ok(mut status) = self.status.lock() {
            *status = json!({"mode":"managed", "phase":phase, "binding":binding});
        }
    }
}
