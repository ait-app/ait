//! Native push delivery boundary; adapters own provider authentication and HTTP.

use std::{fmt, future::Future, pin::Pin};

use serde_json::Value;

/// User-visible content and routing identifiers, without device credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    /// Short notification heading.
    pub title: String,
    /// Plain-text notification body.
    pub body: String,
    /// Host, Workspace and Agent/terminal identities used when opening the notification.
    pub data: Value,
}

/// Safe delivery failures; provider response bodies and credentials must not be logged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DeliveryError {
    /// APNs or FCM permanently invalidated this installation's token.
    #[error("device push token is no longer registered")]
    Unregistered,
    /// Retryable failure with a minimum delay in seconds; zero uses the default backoff.
    #[error("push provider temporarily unavailable")]
    Retryable(u64),
    /// Credentials, payload or target configuration must be corrected before retrying.
    #[error("push provider rejected the notification")]
    Rejected,
    /// No credentials were configured for this native provider.
    #[error("native push provider is not configured")]
    Unconfigured,
}

/// One asynchronous delivery attempt.
pub type DeliveryFuture<'a> = Pin<Box<dyn Future<Output = Result<(), DeliveryError>> + Send + 'a>>;

/// Direct APNs/FCM adapter. Tokens are opaque serialized installation descriptors and secrets.
pub trait PushTransport: fmt::Debug + Send + Sync {
    /// Deliver `notification` to `token`; return a classified failure without disclosing secrets.
    fn send<'a>(&'a self, notification: &'a Notification, token: &'a str) -> DeliveryFuture<'a>;
}
