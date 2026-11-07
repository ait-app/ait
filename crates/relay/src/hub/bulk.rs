//! The `bulk` lane: on the center's `lane` hint the main serve link opens a
//! second link that carries only `k:"download"` sessions, so file data never
//! queues ahead of RPC on one connection. The bulk link resumes like the main
//! link, ends when the center closes it as `idle`, and ends with the main link.

use std::fmt::Debug;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use secrecy::{ExposeSecret, SecretString};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use super::link::Link;
use super::{Connector, Failure, Role, calls, wire};
use crate::validate_ticket;

/// One-use bulk lane serve ticket. The bulk link's `hello.f` is fixed
/// (`credit`, `download`); a welcome that does not enable both ends the link.
#[derive(Debug)]
pub struct Ticket {
    /// One-use ticket from `POST /v1/node-sessions/{id}/hub-tickets` with
    /// `{"ep":"serve","lane":"bulk"}`.
    pub hub_ticket: SecretString,
}

/// Obtains bulk lane tickets for the main link's node session with the owner's
/// current credentials.
#[async_trait::async_trait]
pub trait Tickets: Debug + Send + Sync {
    /// A fresh ticket, or `None` when the center cannot issue one now.
    async fn bulk(&self) -> Option<Ticket>;
}

/// Opens at most one bulk link for a main link, on the center's `lane` hints.
pub(super) struct Lane {
    pub(super) connector: Connector,
    pub(super) tickets: Arc<dyn Tickets>,
    pub(super) url: String,
    /// The main link's token; the bulk link ends with it.
    pub(super) cancel: CancellationToken,
    /// Set while a bulk link exists, connected or resuming.
    pub(super) open: Arc<AtomicBool>,
}

impl Lane {
    /// Open the bulk link unless one exists. Failures are only logged: the center
    /// delivers to the main link when no bulk link connects in time.
    pub(super) fn hint(&self) {
        if self.open.swap(true, Ordering::AcqRel) {
            return;
        }
        let (connector, tickets, url, cancel, open) = (
            self.connector.clone(),
            self.tickets.clone(),
            self.url.clone(),
            self.cancel.clone(),
            self.open.clone(),
        );
        tokio::spawn(
            async move {
                serve(&connector, tickets.as_ref(), &url, &cancel).await;
                open.store(false, Ordering::Release);
            }
            .instrument(tracing::info_span!("hub_bulk_link")),
        );
    }
}

/// Obtain a ticket and keep the bulk link until the center closes it, it fails
/// or the main link ends.
async fn serve(
    connector: &Connector,
    tickets: &dyn Tickets,
    url: &str,
    cancel: &CancellationToken,
) {
    let ticket = tokio::select! {
        () = cancel.cancelled() => return,
        ticket = tickets.bulk() => ticket,
    };
    let Some(Ticket { hub_ticket }) = ticket else {
        tracing::info!("Hub bulk ticket unavailable; downloads stay on the main link");
        return;
    };
    if validate_ticket(hub_ticket.expose_secret()).is_err() {
        tracing::warn!("Hub bulk ticket rejected");
        return;
    }
    // A bulk link carries no center calls.
    let (_, mut calls) = calls::Calls::channel();
    let link = (hub_ticket, Link::new(wire::BULK_FEATURES.to_vec()));
    match connector
        .run(url, link, &Role::Bulk, cancel, &mut calls)
        .await
    {
        Ok(()) => tracing::info!("Hub bulk link closed"),
        Err(Failure::Rejected(code)) if code == "idle" => {
            tracing::info!("Hub bulk link closed as idle");
        }
        Err(error) => tracing::info!(%error, "Hub bulk link ended"),
    }
}
