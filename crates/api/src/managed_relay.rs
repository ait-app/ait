//! Internal capability for a center-managed daemon; never exposed through RPC.
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use host_link::{Binding, BulkTickets, Error, HubTicket, ManagedRelay};
use serde_json::{Value, json};
use uuid::Uuid;

/// Internal exclusive Hub capability issued before the API is shared.
#[derive(Debug, Clone)]
pub struct ManagedRelayHandle {
    pub(super) hub: relay::hub::Connector,
    pub(super) status: Arc<Mutex<Value>>,
}

/// Bulk lane tickets for the hub link, issued under the publication's current JWT.
#[derive(Debug)]
struct Bulk(BulkTickets);

#[async_trait]
impl relay::hub::bulk::Tickets for Bulk {
    async fn bulk(&self) -> Option<relay::hub::bulk::Ticket> {
        let ticket = self.0.request().await.ok()?;
        Some(relay::hub::bulk::Ticket {
            hub_ticket: ticket.ticket,
        })
    }
}

#[async_trait]
impl ManagedRelay for ManagedRelayHandle {
    async fn hub_online(&self) -> bool {
        self.hub.status().await.online
    }

    async fn stop(&self) {
        self.hub.stop().await;
    }

    async fn start_hub(
        &self,
        session: Uuid,
        ticket: HubTicket,
        bulk: Option<BulkTickets>,
    ) -> Result<(), Error> {
        self.hub
            .start(relay::hub::Grant {
                center_url: host_link::center(),
                hub_ticket: ticket.ticket,
                node_session_id: session,
                hub_features: ticket.features,
                tickets: bulk
                    .map(|bulk| Arc::new(Bulk(bulk)) as Arc<dyn relay::hub::bulk::Tickets>),
            })
            .await
            .map_err(|_| Error::Unavailable)
    }

    async fn hub_active(&self) -> bool {
        let status = self.hub.status().await;
        // A link another process took over is left to it for a while.
        status.online || status.connecting || self.hub.yielding().await
    }

    async fn hub_lease(&self) -> bool {
        self.hub.status().await.holds_lease()
    }

    async fn report(&self, phase: &'static str, binding: Option<&Binding>) {
        if let Ok(mut status) = self.status.lock() {
            *status = json!({"mode":"managed", "phase":phase, "binding":binding});
        }
    }
}
