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
    /// Original declaration used for an exchange retry.
    pub machine: Machine,
    /// Last successfully committed refresh credential.
    pub credential: Option<Credential>,
    /// Operation awaiting a durable response.
    pub pending: Option<Pending>,
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
}

/// Sanitized failures: adapters must not embed responses, URLs or credentials.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// Retryable network or center outage.
    #[error("center temporarily unavailable")]
    Unavailable,
    /// Device revoked, expired, or unauthorized. Human reauthorization is required.
    #[error("device authorization is invalid; run daemon login")]
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
