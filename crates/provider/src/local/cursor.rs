//! Cursor CLI sessions over the official ACP stdio protocol.

mod config;
mod discovery;
mod interactions;
mod session;

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

/// Stable Cursor identity for catalogs, configuration and persistence handles.
pub const PROVIDER: &str = "cursor";

/// Cursor-owned CLI authentication and conversations, exposed through ACP.
#[derive(Debug, Clone)]
pub struct CursorClient {
    program: PathBuf,
    deadline: Duration,
    environment: AgentEnvironment,
    images: super::images::ImageStore,
}

impl CursorClient {
    /// Configure `program` without a shell or fallback to another executable.
    /// Returns an adapter with a thirty-second control deadline and unlimited prompt duration.
    #[must_use]
    pub fn new(program: PathBuf) -> Self {
        Self {
            program,
            deadline: Duration::from_secs(30),
            environment: AgentEnvironment::default(),
            images: super::images::ImageStore::default(),
        }
    }

    /// Find Cursor's `cursor-agent` or `agent` CLI on PATH or in its user install directory.
    /// Returns an unavailable launcher when absent; no software or credentials are changed.
    #[must_use]
    pub fn installed() -> Self {
        Self::new(discovery::installed_program())
    }

    /// Use `directory` to materialize native images for live and persisted timelines.
    /// Returns the configured adapter without creating files.
    #[must_use]
    pub fn with_image_directory(mut self, directory: PathBuf) -> Self {
        self.images = super::images::ImageStore::new(directory);
        self
    }

    async fn inspect(
        &self,
        spec: &AgentSessionSpec,
    ) -> Result<session::Session, AgentSessionError> {
        config::validate_spec(spec)?;
        let mut probe = spec.clone();
        probe.config = StoredAgentConfig::default();
        session::open(self, &probe, None).await
    }

    async fn probe(&self, spec: &AgentSessionSpec) -> Result<Details, AgentSessionError> {
        let mut session = self.inspect(spec).await?;
        let details = config::details(&session.options, &session.catalog);
        let closed = session.close().await;
        let details = details?;
        closed?;
        Ok(details)
    }
}

impl AgentClient for CursorClient {
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
        json!({"availableModes":[],"features":[],"capabilities":{
            "supportsStreaming":true,"supportsReasoningStream":true,
            "supportsMcpServers":false,"supportsDynamicModes":true,
            "supportsSessionListing":false,"supportsRewindConversation":false,
            "supportsRewindFiles":false,"supportsRewindBoth":false}})
    }

    fn is_available(&self) -> AgentSessionFuture<'_, bool> {
        Box::pin(async { Ok(super::configuration::executable(&self.program)) })
    }

    fn diagnostic(&self) -> AgentSessionFuture<'_, String> {
        Box::pin(async move {
            if !self.is_available().await? {
                return Ok("Cursor CLI executable is unavailable".to_owned());
            }
            let cwd = std::env::current_dir().map_err(|_| AgentSessionError::Failed)?;
            let details = self
                .discover(cwd.to_str().ok_or(AgentSessionError::Failed)?)
                .await?;
            Ok(format!("Cursor ACP ready; {} models", details.models.len()))
        })
    }

    fn discover<'a>(&'a self, cwd: &'a str) -> AgentSessionFuture<'a, Details> {
        Box::pin(async move {
            self.probe(&AgentSessionSpec {
                provider: PROVIDER.to_owned(),
                cwd: cwd.to_owned(),
                config: StoredAgentConfig::default(),
            })
            .await
        })
    }

    fn validate_selection<'a>(&'a self, spec: &'a AgentSessionSpec) -> AgentSessionFuture<'a, ()> {
        Box::pin(async move {
            let mut session = self.inspect(spec).await?;
            let result =
                config::validate_selection(&session.options, &session.catalog, &spec.config);
            let closed = session.close().await;
            result.and(closed)
        })
    }

    fn draft_features<'a>(
        &'a self,
        spec: &'a AgentSessionSpec,
    ) -> AgentSessionFuture<'a, Vec<Value>> {
        Box::pin(async move {
            let mut session = self.inspect(spec).await?;
            let features = config::features(&session.options, &session.catalog, &spec.config);
            session.close().await?;
            Ok(features)
        })
    }

    fn commands<'a>(&'a self, spec: &'a AgentSessionSpec) -> AgentSessionFuture<'a, Vec<Value>> {
        Box::pin(async move {
            let mut session = self.inspect(spec).await?;
            let result = session.commands().await;
            let closed = session.close().await;
            let commands = result?;
            closed?;
            Ok(commands)
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
