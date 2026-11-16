//! `MiniMax` Code over its official ACP interface, matching Paseo's integration.

mod config;
mod interactions;
mod session;

use std::path::PathBuf;
use std::time::Duration;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};
use serde_json::{Value, json};

use provider_core::ports::agent_session::{
    AgentClient, AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionSpec,
};
use provider_core::ports::environment::AgentEnvironment;
use provider_core::protocol::provider::Details;

const PROVIDER: &str = "minimax-code";

/// Official `MiniMax` Code executable; native credentials and history remain provider-owned.
#[derive(Debug, Clone)]
pub struct MiniMaxClient {
    program: PathBuf,
    deadline: Duration,
    environment: AgentEnvironment,
    images: provider_native::images::ImageStore,
}

impl MiniMaxClient {
    /// Select `program` without shell interpretation or installation side effects.
    /// Control requests are bounded to thirty seconds; inference has no host timeout.
    #[must_use]
    pub fn new(program: PathBuf) -> Self {
        Self {
            program,
            deadline: Duration::from_secs(30),
            environment: AgentEnvironment::default(),
            images: provider_native::images::ImageStore::default(),
        }
    }

    /// Keep submitted and returned images in the host's durable artifact directory.
    #[must_use]
    pub fn with_image_directory(mut self, directory: PathBuf) -> Self {
        self.images = provider_native::images::ImageStore::new(directory);
        self
    }
}

#[async_trait::async_trait]
impl AgentClient for MiniMaxClient {
    fn supports_agent_mcp(&self) -> bool {
        true
    }

    fn supports_write_confinement(&self) -> bool {
        true
    }

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
        config::settings(&json!([]))
    }

    async fn is_available(&self) -> Result<bool, AgentSessionError> {
        Ok(provider_native::configuration::executable(&self.program))
    }

    async fn diagnostic(&self) -> Result<String, AgentSessionError> {
        Ok(if self.is_available().await? {
            "MiniMax Code ACP requires native authentication; run mcode login."
        } else {
            "MiniMax Code executable is unavailable; install @minimax-ai/code \
                 or set AIT_SERVER_MINIMAX_CODE_BIN."
        }
        .to_owned())
    }

    async fn discover(&self, cwd: &str) -> Result<Details, AgentSessionError> {
        let spec = AgentSessionSpec {
            provider: PROVIDER.to_owned(),
            cwd: cwd.to_owned(),
            config: StoredAgentConfig::default(),
            confinement: None,
        };
        let mut session = session::open(self, &spec, None).await?;
        let details = tokio::time::timeout(self.deadline, session.discover())
            .await
            .unwrap_or(Err(AgentSessionError::Failed));
        session.close().await?;
        details
    }

    async fn draft_features(
        &self,
        spec: &AgentSessionSpec,
    ) -> Result<Vec<Value>, AgentSessionError> {
        let mut session = session::open(self, spec, None).await?;
        let features = config::features(&session.options);
        session.close().await?;
        features
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
