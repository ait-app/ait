use chrono::{DateTime, Utc};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stable machine declaration. Names are labels, never identity keys.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Machine {
    /// Persistent installation identity.
    pub server_id: Uuid,
    /// Human-readable label.
    pub display_name: String,
    /// Operating system name.
    pub platform: String,
    /// Running daemon version.
    pub app_version: String,
}

/// Immutable center assignment for one authorization.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Binding {
    /// Assigned node.
    pub node_id: Uuid,
    /// Existing Host record.
    pub host_id: Uuid,
    /// Stable daemon identity.
    pub server_id: Uuid,
    /// Long-term authorization record.
    pub grant_id: Uuid,
}

/// Durable refresh credential; access JWTs are held only in memory.
#[derive(Debug, Clone)]
pub struct Credential {
    /// Rotating opaque refresh secret.
    pub refresh_token: SecretString,
    /// Absolute refresh expiry, independent of process restarts.
    pub refresh_expires_at: DateTime<Utc>,
    /// Immutable authorization target.
    pub binding: Binding,
}

/// In-memory response, including its original absolute expiry during receipt recovery.
#[derive(Debug, Clone)]
pub struct Tokens {
    /// Short-lived device JWT.
    pub access_token: SecretString,
    /// Absolute JWT expiry.
    pub access_expires_at: DateTime<Utc>,
    /// Next durable refresh credential.
    pub credential: Credential,
}

/// Persisted operation identity, written before any consuming HTTP request.
#[derive(Debug, Clone)]
pub enum Pending {
    /// Retrying the same old refresh token and request ID retrieves the same pair.
    Refresh(Uuid),
    /// Single-use enrollment exchange, also recoverable after a process crash.
    Enrollment {
        /// Secret supplied by the operator.
        token: SecretString,
        /// Stable exchange identity.
        request_id: Uuid,
    },
    /// Private Web authorization code, never displayed.
    Web {
        /// Secret polling code.
        device_code: SecretString,
        /// Public confirmation code shown again when the login process resumes.
        user_code: String,
        /// Stable issuance identity.
        request_id: Uuid,
        /// Original deadline, not extended on retries.
        expires_at: DateTime<Utc>,
        /// Current minimum polling interval.
        interval: u64,
    },
}

/// Entire crash-recoverable credential snapshot.
#[derive(Debug, Clone)]
pub struct State {
    /// Explicit sign-out, retained without secrets so clients cannot silently re-enroll on restart.
    pub disconnected: bool,
    /// Last external credential handoff, retained after rotation to make API retries idempotent.
    pub authorization_id: Option<Uuid>,
    /// Original declaration used for an exchange retry.
    pub machine: Machine,
    /// Last successfully committed refresh credential.
    pub credential: Option<Credential>,
    /// Operation awaiting a durable response.
    pub pending: Option<Pending>,
}

impl State {
    /// Remove all authorization and pending issuance/rotation state, retaining only machine identity
    /// and explicit sign-out intent. Returns the old credential for bounded best-effort revocation.
    pub fn disconnect(&mut self) -> Option<Credential> {
        self.disconnected = true;
        self.authorization_id = None;
        self.pending = None;
        self.credential.take()
    }
}

/// Active runtime lease, distinct from long-term authorization.
#[derive(Debug, Clone)]
pub struct Session {
    /// Center-issued running session ID.
    pub id: Uuid,
    /// Initial lease end, derived from the registration response.
    pub lease_until: DateTime<Utc>,
    /// Suggested renewal interval.
    pub renew_after_seconds: u64,
    /// A serve ticket issued with the registration, saving the first ticket request.
    pub hub_ticket: Option<HubTicket>,
}

/// One-use `ait-hub-v1` serve credential and the features declared with it.
#[derive(Debug, Clone)]
pub struct HubTicket {
    /// One-use 64-hex ticket for `/v1/hub`.
    pub ticket: SecretString,
    /// `hub_features` the center declared with the ticket. The Hub link refuses
    /// to start when one it needs is missing.
    pub features: Vec<String>,
    /// Local expiry of a ticket issued ahead of use; `None` when used at once.
    pub expires_at: Option<tokio::time::Instant>,
}

impl HubTicket {
    /// A ticket for immediate use.
    #[must_use]
    pub fn new(ticket: SecretString, features: Vec<String>) -> Self {
        Self {
            ticket,
            features,
            expires_at: None,
        }
    }

    /// Whether the ticket can still be presented.
    #[must_use]
    pub fn usable(&self) -> bool {
        self.expires_at
            .is_none_or(|expires| tokio::time::Instant::now() < expires)
    }
}

/// A hub link's request for a `bulk` lane serve ticket of `session`.
#[derive(Debug)]
pub(crate) struct BulkRequest {
    pub(crate) session: Uuid,
    pub(crate) reply: tokio::sync::oneshot::Sender<Result<HubTicket, Error>>,
}

/// Lets the hub link of one node session obtain `bulk` lane serve tickets on
/// demand. The controller answers with its current JWT while that session is
/// still the published one.
#[derive(Debug, Clone)]
pub struct BulkTickets {
    requests: tokio::sync::mpsc::Sender<BulkRequest>,
    session: Uuid,
}

impl BulkTickets {
    pub(crate) const fn new(
        requests: tokio::sync::mpsc::Sender<BulkRequest>,
        session: Uuid,
    ) -> Self {
        Self { requests, session }
    }

    /// Request a one-use `bulk` lane serve ticket.
    /// # Errors
    /// Returns `Unavailable` once the controller stopped or published another
    /// session, or the center's refusal.
    pub async fn request(&self) -> Result<HubTicket, Error> {
        let (reply, response) = tokio::sync::oneshot::channel();
        self.requests
            .send(BulkRequest {
                session: self.session,
                reply,
            })
            .await
            .map_err(|_| Error::Unavailable)?;
        response.await.map_err(|_| Error::Unavailable)?
    }
}

/// Sanitized failures: adapters must not embed responses, URLs or credentials.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// Retryable network or center outage.
    #[error("center temporarily unavailable")]
    Unavailable,
    /// A short-lived access JWT was refused; probe the machine refresh credential once.
    #[error("access token was refused")]
    AccessRejected,
    /// Device revoked, expired, or unauthorized. Human reauthorization is required.
    #[error("device authorization is invalid; run aitd login")]
    Unauthorized,
    /// A running instance already owns this identity.
    #[error("Host runtime conflict; stop the other instance or wait for its lease")]
    Conflict,
    /// Session expired, requiring registration again.
    #[error("node session expired")]
    SessionExpired,
    /// Untrusted or inconsistent response.
    #[error("center protocol rejected")]
    Protocol,
    /// Durable state could not be committed. Rotation must pause.
    #[error("private device state could not be saved")]
    Storage,
    /// Web approval still pending.
    #[error("authorization pending")]
    Pending,
    /// Center requires a longer polling interval.
    #[error("authorization polling slowed down")]
    SlowDown,
    /// Human denied or authorization code expired.
    #[error("Web authorization denied or expired")]
    Denied,
}
