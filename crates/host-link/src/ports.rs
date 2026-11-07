use async_trait::async_trait;
use chrono::{DateTime, Utc};
use secrecy::SecretString;
use uuid::Uuid;

use crate::{Binding, BulkTickets, Error, HubTicket, Machine, Session, State, Tokens};

/// Private snapshot storage. Implementations synchronize files and parent directories.
pub trait CredentialStore: Send + Sync {
    /// Commit a complete snapshot atomically before returning success.
    /// # Errors
    /// Returns `Storage` if durability cannot be established.
    fn save(&self, state: &State) -> Result<(), Error>;
}

/// Scoped center operations; never a user/account API client.
#[async_trait]
pub trait Center: Send + Sync {
    /// Redeem a one-use enrollment with a durable request identity.
    /// # Errors
    /// Returns authorization, protocol or retryable transport failures.
    async fn enroll(
        &self,
        token: &SecretString,
        request_id: Uuid,
        machine: &Machine,
    ) -> Result<Tokens, Error>;
    /// Claim a previously approved device authorization.
    /// # Errors
    /// Distinguishes pending/slow polling from denied or unavailable authorization.
    async fn poll(&self, code: &SecretString, request_id: Uuid) -> Result<Tokens, Error>;
    /// Rotate with a persisted operation identity.
    /// # Errors
    /// Distinguishes terminal authorization errors from retryable outages.
    async fn refresh(&self, secret: &SecretString, request_id: Uuid) -> Result<Tokens, Error>;
    /// Register this daemon instance using the short-lived JWT.
    /// # Errors
    /// Rejects conflicts and inconsistent center assignments.
    async fn register(
        &self,
        tokens: &Tokens,
        machine: &Machine,
        instance: Uuid,
        registration: Uuid,
    ) -> Result<Session, Error>;
    /// Renew a runtime lease with the latest JWT.
    /// # Errors
    /// Returns authorization or session expiry failures without extending the lease locally.
    async fn renew(&self, tokens: &Tokens, session: Uuid) -> Result<DateTime<Utc>, Error>;
    /// Obtain a one-use hub serve ticket for this runtime.
    /// # Errors
    /// Rejects expired or unauthorized sessions, and responses without `hub_features`.
    async fn hub_ticket(&self, tokens: &Tokens, session: Uuid) -> Result<HubTicket, Error>;
    /// Obtain a one-use hub serve ticket for this runtime's download-only `bulk` lane.
    /// # Errors
    /// Rejects expired or unauthorized sessions, and responses without `hub_features`.
    async fn hub_bulk_ticket(&self, tokens: &Tokens, session: Uuid) -> Result<HubTicket, Error>;
    /// Revoke the shared machine authorization on explicit disconnect.
    /// # Errors
    /// May fail offline; local credential removal must still complete without retaining a retry secret.
    async fn revoke(&self, refresh_token: &SecretString) -> Result<(), Error>;
    /// Release a runtime without revoking its machine grant.
    /// # Errors
    /// May fail during center outages; leases still expire remotely.
    async fn close(&self, tokens: &Tokens, session: Uuid) -> Result<(), Error>;
}

/// Exclusive internal Hub ownership. External adapters control the shared publication
/// lifecycle; they cannot install independent transport grants while it is claimed.
#[async_trait]
pub trait ManagedRelay: Send + Sync {
    /// Close the Hub link and every session it carries immediately.
    async fn stop(&self);
    /// Begin the hub serve link with an ephemeral ticket. `bulk` obtains tickets
    /// for the link's `bulk` lane; without it the link never declares that feature.
    /// # Errors
    /// Rejects invalid tickets, tickets whose `hub_features` lack a feature the
    /// link needs, or unavailable local transport.
    async fn start_hub(
        &self,
        session: Uuid,
        ticket: HubTicket,
        bulk: Option<BulkTickets>,
    ) -> Result<(), Error>;
    /// Return whether the hub link is online, connecting, or yielding to another owner.
    async fn hub_active(&self) -> bool;
    /// Return whether the Hub handshake has succeeded, for resetting retry backoff.
    async fn hub_online(&self) -> bool;
    /// Return whether a hub link exists, attached or resuming: it then holds the node
    /// session lease (`lease` feature), so the lease must not be renewed over HTTP.
    async fn hub_lease(&self) -> bool;
    /// Publish a sanitized manager state for local status queries.
    /// Waits for best-effort persistence without blocking the async runtime.
    async fn report(&self, phase: &'static str, binding: Option<&Binding>);
}
