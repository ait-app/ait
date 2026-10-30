use std::future::Future;

use chrono::{DateTime, Utc};
use secrecy::SecretString;
use uuid::Uuid;

use crate::{Binding, Error, Machine, Session, State, Tokens};

/// Private snapshot storage. Implementations synchronize files and parent directories.
pub trait CredentialStore: Send + Sync {
    /// Commit a complete snapshot atomically before returning success.
    /// # Errors
    /// Returns `Storage` if durability cannot be established.
    fn save(&self, state: &State) -> Result<(), Error>;
}

/// Scoped center operations; never a user/account API client.
pub trait Center: Send + Sync {
    /// Redeem a one-use enrollment with a durable request identity.
    /// # Errors
    /// Returns authorization, protocol or retryable transport failures.
    fn enroll(
        &self,
        token: &SecretString,
        request_id: Uuid,
        machine: &Machine,
    ) -> impl Future<Output = Result<Tokens, Error>> + Send;
    /// Claim a previously approved device authorization.
    /// # Errors
    /// Distinguishes pending/slow polling from denied or unavailable authorization.
    fn poll(
        &self,
        code: &SecretString,
        request_id: Uuid,
    ) -> impl Future<Output = Result<Tokens, Error>> + Send;
    /// Rotate with a persisted operation identity.
    /// # Errors
    /// Distinguishes terminal authorization errors from retryable outages.
    fn refresh(
        &self,
        secret: &SecretString,
        request_id: Uuid,
    ) -> impl Future<Output = Result<Tokens, Error>> + Send;
    /// Register this daemon instance using the short-lived JWT.
    /// # Errors
    /// Rejects conflicts and inconsistent center assignments.
    fn register(
        &self,
        tokens: &Tokens,
        machine: &Machine,
        instance: Uuid,
        registration: Uuid,
    ) -> impl Future<Output = Result<Session, Error>> + Send;
    /// Renew a runtime lease with the latest JWT.
    /// # Errors
    /// Returns authorization or session expiry failures without extending the lease locally.
    fn renew(
        &self,
        tokens: &Tokens,
        session: Uuid,
    ) -> impl Future<Output = Result<DateTime<Utc>, Error>> + Send;
    /// Obtain a one-use control ticket for this runtime.
    /// # Errors
    /// Rejects expired or unauthorized sessions.
    fn ticket(
        &self,
        tokens: &Tokens,
        session: Uuid,
    ) -> impl Future<Output = Result<SecretString, Error>> + Send;
    /// Obtain a one-use hub serve ticket for this runtime. The hub is optional;
    /// centers without it report `Unavailable`.
    /// # Errors
    /// Rejects expired or unauthorized sessions, and centers without a hub.
    fn hub_ticket(
        &self,
        _tokens: &Tokens,
        _session: Uuid,
    ) -> impl Future<Output = Result<SecretString, Error>> + Send {
        std::future::ready(Err(Error::Unavailable))
    }
    /// Revoke the shared machine authorization on explicit disconnect.
    /// # Errors
    /// May fail offline; local credential removal must still complete without retaining a retry secret.
    fn revoke(
        &self,
        refresh_token: &SecretString,
    ) -> impl Future<Output = Result<(), Error>> + Send;
    /// Release a runtime without revoking its machine grant.
    /// # Errors
    /// May fail during center outages; leases still expire remotely.
    fn close(
        &self,
        tokens: &Tokens,
        session: Uuid,
    ) -> impl Future<Output = Result<(), Error>> + Send;
}

/// Exclusive internal Relay/Hub ownership. External adapters control the shared publication
/// lifecycle; they cannot install independent transport grants while it is claimed.
pub trait ManagedRelay: Send + Sync {
    /// Begin a control connection with an ephemeral ticket from the fixed authority.
    /// # Errors
    /// Rejects invalid tickets or unavailable local transport.
    fn start(
        &self,
        session: Uuid,
        ticket: SecretString,
    ) -> impl Future<Output = Result<(), Error>> + Send;
    /// Return whether a connection is online or connecting.
    fn active(&self) -> impl Future<Output = bool> + Send;
    /// Return whether the Relay handshake has succeeded (connecting alone is not success).
    fn online(&self) -> impl Future<Output = bool> + Send;
    /// Close Relay, Hub and all reverse data channels immediately.
    fn stop(&self) -> impl Future<Output = ()> + Send;
    /// Begin the optional hub serve link with an ephemeral ticket.
    /// # Errors
    /// Rejects invalid tickets or unavailable local transport.
    fn start_hub(
        &self,
        session: Uuid,
        ticket: SecretString,
    ) -> impl Future<Output = Result<(), Error>> + Send;
    /// Return whether the hub link is online, connecting, or yielding to another owner.
    fn hub_active(&self) -> impl Future<Output = bool> + Send;
    /// Return whether the Hub handshake has succeeded, for resetting retry backoff.
    fn hub_online(&self) -> impl Future<Output = bool> + Send;
    /// Publish a sanitized manager state for local status queries.
    fn report(&self, phase: &'static str, binding: Option<&Binding>);
}
