//! Oh My Pi's native JSONL RPC adapter. OMP owns credentials, extensions, tools and session files.

mod config;
mod framing;
mod history;
mod interactions;
mod session;
mod stream;
mod subagents;
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
use crate::ports::native_history::{ListOptions, SessionDescriptor, SessionHistory};
use crate::protocol::provider::Details;

/// Installed OMP executable with bounded RPC startup and discovery.
#[derive(Debug, Clone)]
pub(crate) struct OmpClient {
    program: PathBuf,
    deadline: Duration,
    environment: AgentEnvironment,
    images: super::images::ImageStore,
}

impl OmpClient {
    /// Use `program` directly, inheriting the local OMP configuration and authentication.
    #[must_use]
    pub(crate) fn new(program: PathBuf) -> Self {
        Self {
            program,
            deadline: Duration::from_secs(30),
            environment: AgentEnvironment::default(),
            images: super::images::ImageStore::default(),
        }
    }

    /// Store rendered native image output in the daemon's private `directory`.
    #[must_use]
    pub(crate) fn with_image_directory(mut self, directory: PathBuf) -> Self {
        self.images = super::images::ImageStore::new(directory);
        self
    }
}

impl AgentClient for OmpClient {
    fn provider(&self) -> &'static str {
        "omp"
    }

    fn supports_session_import(&self) -> bool {
        true
    }

    fn validate_config(&self, config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        config::validate(config)
    }

    fn settings(&self, config: &StoredAgentConfig) -> Value {
        json!({"availableModes":config::modes(),"features":config::features(config),"capabilities":{
            "supportsStreaming":true,"supportsReasoningStream":true,
            "supportsSessionPersistence":true,"supportsSessionListing":true,
            "supportsToolInvocations":true,"supportsMcpServers":false,
            "supportsDynamicModes":false,"supportsRewindConversation":false,
            "supportsRewindFiles":false,"supportsRewindBoth":false}})
    }

    fn is_available(&self) -> AgentSessionFuture<'_, bool> {
        Box::pin(async { Ok(super::configuration::executable(&self.program)) })
    }

    fn discover<'a>(&'a self, cwd: &'a str) -> AgentSessionFuture<'a, Details> {
        Box::pin(async move {
            let mut probe = session::probe(self, cwd).await?;
            let result = config::discover(&mut probe).await;
            let closed = probe.close().await;
            let details = result?;
            closed?;
            Ok(details)
        })
    }

    fn commands<'a>(&'a self, spec: &'a AgentSessionSpec) -> AgentSessionFuture<'a, Vec<Value>> {
        Box::pin(async move {
            config::validate_spec(spec)?;
            let mut probe = session::probe(self, &spec.cwd).await?;
            let result = probe.request("get_available_commands", json!({})).await;
            let closed = probe.close().await;
            let result = result?;
            closed?;
            config::commands(&result)
        })
    }

    fn diagnostic(&self) -> AgentSessionFuture<'_, String> {
        Box::pin(async move {
            if !self.is_available().await? {
                return Ok(
                    "Oh My Pi executable is unavailable; check PATH or AIT_SERVER_OMP_BIN."
                        .to_owned(),
                );
            }
            let cwd = std::env::current_dir().map_err(|_| AgentSessionError::Failed)?;
            let details = self
                .discover(cwd.to_str().ok_or(AgentSessionError::Failed)?)
                .await?;
            Ok(format!(
                "Oh My Pi RPC ready; {} available models",
                details.models.len()
            ))
        })
    }

    fn history<'a>(
        &'a self,
        handle: &'a AgentPersistenceHandle,
        cwd: &'a str,
    ) -> AgentSessionFuture<'a, Vec<crate::protocol::timeline::NativeItem>> {
        Box::pin(async move { Ok(history::inspect(self, handle, cwd).await?.entries) })
    }

    fn inspect_session<'a>(
        &'a self,
        handle: &'a AgentPersistenceHandle,
        cwd: &'a str,
    ) -> AgentSessionFuture<'a, SessionHistory> {
        Box::pin(history::inspect(self, handle, cwd))
    }

    fn list_sessions<'a>(
        &'a self,
        options: &'a ListOptions,
    ) -> AgentSessionFuture<'a, Vec<SessionDescriptor>> {
        Box::pin(history::list(self, options))
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
        let mut values: std::collections::BTreeMap<String, String> = client
            .environment
            .entries()
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect();
        values.extend(
            environment
                .entries()
                .map(|(key, value)| (key.to_owned(), value.to_owned())),
        );
        Box::pin(async move {
            client.environment = AgentEnvironment::try_from(values)?;
            client.create_session(spec).await
        })
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
