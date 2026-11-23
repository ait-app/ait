//! Codex's upstream WebRTC peer. Neither SDP nor upstream addresses cross the provider port.
//!
//! The peer and its Opus codec are compiled only with the default `voice` feature. Without it,
//! `open` rejects and the adapter does not advertise realtime voice.

#[cfg(feature = "voice")]
mod codec;
#[cfg(feature = "voice")]
mod peer;

use std::{future::Future, pin::Pin};

use model::realtime::Endpoint;
use tokio::sync::{oneshot, watch};
use tokio_util::sync::CancellationToken;

use super::AgentSessionError;

/// Provider-owned media constructor; tests can replace the external transport boundary.
pub(crate) type Factory =
    fn(Endpoint) -> Pin<Box<dyn Future<Output = Result<Media, AgentSessionError>> + Send>>;

/// A cancellable provider-owned peer with native negotiation and interruption controls.
#[derive(Debug)]
pub(crate) struct Media {
    pub(super) offer: String,
    answer: Option<oneshot::Sender<String>>,
    cancel: CancellationToken,
    interruptions: watch::Sender<()>,
}

impl Drop for Media {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl Media {
    /// Discard buffered playback once on interruption, without gating subsequent reply audio.
    pub(super) fn interrupt(&self) {
        self.interruptions.send_replace(());
    }

    /// Apply the single native SDP answer, failing if startup was cancelled or already answered.
    pub(super) fn answer(&mut self, sdp: String) -> Result<(), AgentSessionError> {
        self.answer
            .take()
            .ok_or(AgentSessionError::Failed)?
            .send(sdp)
            .map_err(|_| AgentSessionError::Failed)
    }
}

/// Gather an upstream offer within the caller lifetime and bounded startup deadline.
#[cfg(feature = "voice")]
pub(crate) fn open(
    endpoint: Endpoint,
) -> Pin<Box<dyn Future<Output = Result<Media, AgentSessionError>> + Send>> {
    Box::pin(async move {
        let cancel = endpoint.cancel.clone();
        let result = tokio::select! {
            () = cancel.cancelled() => return Err(AgentSessionError::Failed),
            result = tokio::time::timeout(std::time::Duration::from_secs(10), peer::prepare(endpoint)) => result,
        };
        result.map_err(|_| AgentSessionError::Failed)?
    })
}

/// Reject realtime voice in builds without the `voice` feature.
#[cfg(not(feature = "voice"))]
pub(crate) fn open(
    _endpoint: Endpoint,
) -> Pin<Box<dyn Future<Output = Result<Media, AgentSessionError>> + Send>> {
    Box::pin(std::future::ready(Err(AgentSessionError::Rejected)))
}

#[cfg(all(unix, any(test, feature = "test-support")))]
pub(crate) mod offline;
