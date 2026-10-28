//! `MiniMax` Code over its official ACP interface, matching Paseo's integration.

mod config;
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

impl AgentClient for MiniMaxClient {
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

    fn is_available(&self) -> AgentSessionFuture<'_, bool> {
        Box::pin(async { Ok(super::configuration::executable(&self.program)) })
    }

    fn diagnostic(&self) -> AgentSessionFuture<'_, String> {
        Box::pin(async {
            Ok(if self.is_available().await? {
                "MiniMax Code ACP requires native authentication; run mcode login."
            } else {
                "MiniMax Code executable is unavailable; install @minimax-ai/code \
                 or set AIT_SERVER_MINIMAX_CODE_BIN."
            }
            .to_owned())
        })
    }

    fn discover<'a>(&'a self, cwd: &'a str) -> AgentSessionFuture<'a, Details> {
        Box::pin(async move {
            let spec = AgentSessionSpec {
                provider: PROVIDER.to_owned(),
                cwd: cwd.to_owned(),
                config: StoredAgentConfig::default(),
            };
            let mut session = session::open(self, &spec, None).await?;
            let details = config::details(&session.options);
            session.close().await?;
            details
        })
    }

    fn draft_features<'a>(
        &'a self,
        spec: &'a AgentSessionSpec,
    ) -> AgentSessionFuture<'a, Vec<Value>> {
        Box::pin(async move {
            let mut session = session::open(self, spec, None).await?;
            let features = config::features(&session.options);
            session.close().await?;
            features
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
