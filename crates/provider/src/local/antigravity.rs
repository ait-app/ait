//! Google Antigravity's official `agy` CLI over its headless NDJSON protocol.

mod config;
mod diagnostics;
mod discovery;
mod input;
mod quota;
mod session;
mod streaming;
mod transport;

use std::path::PathBuf;
use std::time::Duration;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};
use serde_json::{Value, json};

use crate::ports::agent_session::{
    AgentClient, AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionSpec,
};
use crate::ports::environment::AgentEnvironment;
use crate::protocol::provider::Details;

/// Stable provider identity, independent of the executable's `agy` name.
const PROVIDER: &str = "antigravity";

/// Installed native CLI; credentials, permissions and conversation storage remain AGY-owned.
#[derive(Debug, Clone)]
pub(crate) struct AntigravityClient {
    program: PathBuf,
    deadline: Duration,
    environment: AgentEnvironment,
    images: super::images::ImageStore,
}

impl AntigravityClient {
    /// Use `program` directly without a shell or fallback to another installed executable.
    ///
    /// Startup and discovery are bounded to thirty seconds; foreground turns have no timeout.
    #[must_use]
    pub(crate) fn new(program: PathBuf) -> Self {
        Self {
            program,
            deadline: Duration::from_secs(30),
            environment: AgentEnvironment::default(),
            images: super::images::ImageStore::default(),
        }
    }

    /// Keep submitted images in the host's private artifact directory for native reads and history.
    #[must_use]
    pub(crate) fn with_image_directory(mut self, directory: PathBuf) -> Self {
        self.images = super::images::ImageStore::new(directory);
        self
    }

    /// Find `agy` on PATH, then in official installer and Homebrew locations.
    ///
    /// Returns an unavailable launcher when no executable exists. Does not install software.
    #[must_use]
    pub(crate) fn installed() -> Self {
        Self::new(discovery::installed_program())
    }
}

#[async_trait::async_trait]
impl AgentClient for AntigravityClient {
    fn provider(&self) -> &'static str {
        PROVIDER
    }

    fn supports_history_replay(&self) -> bool {
        false
    }

    fn validate_config(&self, config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        config::validate(config)
    }

    fn settings(&self, _config: &StoredAgentConfig) -> Value {
        json!({"availableModes":config::modes(),"features":[],"capabilities":{
            "supportsStreaming":true,"supportsReasoningStream":false,
            "supportsMcpServers":false,"supportsDynamicModes":false,
            "supportsSessionListing":false,"supportsRewindConversation":false,
            "supportsRewindFiles":false,"supportsRewindBoth":false}})
    }

    async fn is_available(&self) -> Result<bool, AgentSessionError> {
        Ok(super::configuration::executable(&self.program))
    }

    async fn diagnostic(&self) -> Result<String, AgentSessionError> {
        if !self.is_available().await? {
            return Ok("Antigravity CLI (agy) executable is unavailable".to_owned());
        }
        let cwd = std::env::current_dir().map_err(|_| AgentSessionError::Failed)?;
        let models =
            discovery::models(self, cwd.to_str().ok_or(AgentSessionError::Failed)?).await?;
        Ok(format!("Antigravity CLI ready; {} models", models.len()))
    }

    async fn usage(&self) -> Result<Value, AgentSessionError> {
        let cwd = std::env::current_dir().map_err(|_| AgentSessionError::Failed)?;
        quota::read(self, &cwd).await
    }

    async fn discover(&self, cwd: &str) -> Result<Details, AgentSessionError> {
        config::validate_directory(cwd)?;
        Ok(Details {
            models: discovery::models(self, cwd).await?,
            modes: config::modes(),
            features: Vec::new(),
        })
    }

    async fn validate_selection(&self, spec: &AgentSessionSpec) -> Result<(), AgentSessionError> {
        config::validate_spec(spec)?;
        if let Some(model) = &spec.config.model
            && !discovery::models(self, &spec.cwd)
                .await?
                .iter()
                .any(|entry| entry["id"] == *model)
        {
            return Err(AgentSessionError::Rejected);
        }
        Ok(())
    }

    async fn create_session(
        &self,
        spec: &AgentSessionSpec,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        Ok(Box::new(session::open(self, spec, None).await?) as Box<dyn AgentSession>)
    }

    async fn create_session_with_environment(
        &self,
        spec: &AgentSessionSpec,
        environment: &AgentEnvironment,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        let mut client = self.clone();
        client.environment = environment.clone();
        client.create_session(spec).await
    }

    async fn resume_session(
        &self,
        handle: &AgentPersistenceHandle,
        spec: &AgentSessionSpec,
        purpose: AgentResumePurpose,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        if purpose == AgentResumePurpose::History {
            return Err(AgentSessionError::Unavailable);
        }
        Ok(Box::new(session::open(self, spec, Some(handle)).await?) as Box<dyn AgentSession>)
    }
}

#[cfg(all(test, unix))]
mod tests;
