//! `MiniMax` Code over its official ACP interface, matching Paseo's integration.

mod config;
mod session;

use std::path::PathBuf;
use std::time::Duration;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};
use serde_json::{Value, json};

use crate::ports::agent_session::{
    AgentClient, AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionSpec,
};
use crate::ports::environment::AgentEnvironment;
use crate::protocol::provider::Details;

const PROVIDER: &str = "minimax-code";

/// Official `MiniMax` Code executable; native credentials and history remain provider-owned.
#[derive(Debug, Clone)]
pub(crate) struct MiniMaxClient {
    program: PathBuf,
    deadline: Duration,
    environment: AgentEnvironment,
    images: super::images::ImageStore,
}

impl MiniMaxClient {
    /// Select `program` without shell interpretation or installation side effects.
    /// Control requests are bounded to thirty seconds; inference has no host timeout.
    pub(crate) fn new(program: PathBuf) -> Self {
        Self {
            program,
            deadline: Duration::from_secs(30),
            environment: AgentEnvironment::default(),
            images: super::images::ImageStore::default(),
        }
    }

    /// Keep submitted and returned images in the host's durable artifact directory.
    pub(crate) fn with_image_directory(mut self, directory: PathBuf) -> Self {
        self.images = super::images::ImageStore::new(directory);
        self
    }
}

#[async_trait::async_trait]
impl AgentClient for MiniMaxClient {
    fn supports_agent_mcp(&self) -> bool {
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
        Ok(super::configuration::executable(&self.program))
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
