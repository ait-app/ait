//! Google Antigravity's official `agy` CLI over its headless NDJSON protocol.

mod config;
mod diagnostics;
mod discovery;
mod session;
mod streaming;
mod transport;

use std::path::PathBuf;
use std::time::Duration;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};
use serde_json::{Value, json};

use crate::ports::agent_session::{
    AgentClient, AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionFuture,
    AgentSessionSpec,
};
use crate::ports::environment::AgentEnvironment;
use crate::protocol::provider::Details;

/// Stable provider identity, independent of the executable's `agy` name.
pub const PROVIDER: &str = "antigravity";

/// Installed native CLI; credentials, permissions and conversation storage remain AGY-owned.
#[derive(Debug, Clone)]
pub struct AntigravityClient {
    program: PathBuf,
    deadline: Duration,
    environment: AgentEnvironment,
}

impl AntigravityClient {
    /// Use `program` directly without a shell or fallback to another installed executable.
    ///
    /// Startup and discovery are bounded to thirty seconds; foreground turns have no timeout.
    #[must_use]
    pub fn new(program: PathBuf) -> Self {
        Self {
            program,
            deadline: Duration::from_secs(30),
            environment: AgentEnvironment::default(),
        }
    }

    /// Find `agy` on PATH, then in official installer and Homebrew locations.
    ///
    /// Returns an unavailable launcher when no executable exists. Does not install software.
    #[must_use]
    pub fn installed() -> Self {
        Self::new(discovery::installed_program())
    }
}

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

    fn is_available(&self) -> AgentSessionFuture<'_, bool> {
        Box::pin(async { Ok(super::configuration::executable(&self.program)) })
    }

    fn diagnostic(&self) -> AgentSessionFuture<'_, String> {
        Box::pin(async move {
            if !self.is_available().await? {
                return Ok("Antigravity CLI (agy) executable is unavailable".to_owned());
            }
            let cwd = std::env::current_dir().map_err(|_| AgentSessionError::Failed)?;
            let models =
                discovery::models(self, cwd.to_str().ok_or(AgentSessionError::Failed)?).await?;
            Ok(format!("Antigravity CLI ready; {} models", models.len()))
        })
    }

    fn discover<'a>(&'a self, cwd: &'a str) -> AgentSessionFuture<'a, Details> {
        Box::pin(async move {
            config::validate_directory(cwd)?;
            Ok(Details {
                models: discovery::models(self, cwd).await?,
                modes: config::modes(),
                features: Vec::new(),
            })
        })
    }

    fn validate_selection<'a>(&'a self, spec: &'a AgentSessionSpec) -> AgentSessionFuture<'a, ()> {
        Box::pin(async move {
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
        })
    }

    fn create_session<'a>(
        &'a self,
        spec: &'a AgentSessionSpec,
    ) -> AgentSessionFuture<'a, Box<dyn AgentSession>> {
        Box::pin(async move {
            Ok(Box::new(session::open(self, spec, None).await?) as Box<dyn AgentSession>)
        })
    }

    fn create_session_with_environment<'a>(
        &'a self,
        spec: &'a AgentSessionSpec,
        environment: &'a AgentEnvironment,
    ) -> AgentSessionFuture<'a, Box<dyn AgentSession>> {
        let mut client = self.clone();
        client.environment = environment.clone();
        Box::pin(async move { client.create_session(spec).await })
    }

    fn resume_session<'a>(
        &'a self,
        handle: &'a AgentPersistenceHandle,
        spec: &'a AgentSessionSpec,
        purpose: AgentResumePurpose,
    ) -> AgentSessionFuture<'a, Box<dyn AgentSession>> {
        Box::pin(async move {
            if purpose == AgentResumePurpose::History {
                return Err(AgentSessionError::Unavailable);
            }
            Ok(Box::new(session::open(self, spec, Some(handle)).await?) as Box<dyn AgentSession>)
        })
    }
}

#[cfg(all(test, unix))]
mod tests;
