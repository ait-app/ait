//! Speech engines and native Agent execution are replaceable external boundaries.

use tokio_util::sync::CancellationToken;

use crate::{Error, audio::Audio};

/// Recognized text and optional provider metadata.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    /// Recognized text; an empty string represents no speech.
    pub text: String,
    /// Detected or configured language.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

/// Speech-to-text boundary. Implementations must honor cancellation and bound output.
#[async_trait::async_trait]
pub trait Transcriber: Send + Sync + std::fmt::Debug {
    /// Check readiness before recording.
    /// # Errors
    /// Returns a retryable error while models are preparing or a backend setup error.
    fn readiness(&self) -> Result<(), Error> {
        Ok(())
    }
    /// Recognize audio, returning text or a safe configuration/provider/cancellation error.
    async fn transcribe(
        &self,
        audio: Audio,
        cancel: CancellationToken,
    ) -> Result<Transcript, Error>;
}

/// Text-to-speech boundary. Implementations return mono PCM16 audio.
#[async_trait::async_trait]
pub trait Synthesizer: Send + Sync + std::fmt::Debug {
    /// Check readiness before enabling conversations.
    /// # Errors
    /// Returns a retryable error while models are preparing or a backend setup error.
    fn readiness(&self) -> Result<(), Error> {
        Ok(())
    }
    /// Synthesize text, returning audio or a safe configuration/provider/cancellation error.
    async fn synthesize(&self, text: &str, cancel: CancellationToken) -> Result<Audio, Error>;
}

/// Native Agent coordination supplied by the composition root.
#[async_trait::async_trait]
pub trait Agents: Send + Sync + std::fmt::Debug {
    /// Open provider-native audio when supported; errors must never trigger text fallback.
    /// Dropping the returned session cancels provider work, including abandoned startup.
    async fn realtime(
        &self,
        _agent: &str,
        _cancel: CancellationToken,
    ) -> Result<Option<model::realtime::Session>, Error> {
        Ok(None)
    }

    /// Resolve and validate an enabled voice target, returning its canonical identity.
    async fn resolve(&self, identifier: &str) -> Result<String, Error>;
    /// Execute a text turn and return its final response. Cancellation must interrupt owned work
    /// and await its cleanup before this operation returns.
    async fn turn(
        &self,
        agent: &str,
        text: &str,
        cancel: CancellationToken,
    ) -> Result<String, Error>;
}
