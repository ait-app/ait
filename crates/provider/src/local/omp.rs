//! Oh My Pi's native JSONL RPC adapter. OMP owns credentials, extensions, tools and session files.

mod bridge;
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
    AgentClient, AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionSpec,
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

    async fn service_tiers(
        &self,
        cwd: &str,
        model: Option<&str>,
    ) -> Result<config::ServiceTiers, AgentSessionError> {
        let mut probe = session::probe(self, cwd, model).await?;
        let result = config::service_tiers(&mut probe).await;
        let closed = probe.close().await;
        let tiers = result?;
        closed?;
        Ok(tiers)
    }
}

/// Serve the OMP bridge on a caller-owned JSONL stream without altering native installation.
/// Returns bounded protocol, process startup, or I/O errors; EOF closes the native runtime.
pub(crate) async fn serve_acp<R, W>(program: PathBuf, input: R, output: W) -> std::io::Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    bridge::serve(OmpClient::new(program), input, output).await
}

#[async_trait::async_trait]
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
        json!({"availableModes":config::modes(),"features":config::features(config, config::ServiceTiers::default()),"capabilities":{
            "supportsStreaming":true,"supportsReasoningStream":true,
            "supportsSessionPersistence":true,"supportsSessionListing":true,
            "supportsToolInvocations":true,"supportsMcpServers":false,
            "supportsDynamicModes":false,"supportsRewindConversation":false,
            "supportsRewindFiles":false,"supportsRewindBoth":false}})
    }

    async fn is_available(&self) -> Result<bool, AgentSessionError> {
        Ok(super::configuration::executable(&self.program))
    }

    async fn discover(&self, cwd: &str) -> Result<Details, AgentSessionError> {
        let mut probe = session::probe(self, cwd, None).await?;
        let result = config::discover(&mut probe).await;
        let closed = probe.close().await;
        let details = result?;
        closed?;
        Ok(details)
    }

    async fn draft_features(
        &self,
        spec: &AgentSessionSpec,
    ) -> Result<Vec<Value>, AgentSessionError> {
        config::validate_spec(spec)?;
        let tiers = self
            .service_tiers(&spec.cwd, spec.config.model.as_deref())
            .await?;
        Ok(config::features(&spec.config, tiers))
    }

    async fn commands(&self, spec: &AgentSessionSpec) -> Result<Vec<Value>, AgentSessionError> {
        config::validate_spec(spec)?;
        let mut probe = session::probe(self, &spec.cwd, None).await?;
        let result = probe.request("get_available_commands", json!({})).await;
        let closed = probe.close().await;
        let result = result?;
        closed?;
        config::commands(&result)
    }

    async fn diagnostic(&self) -> Result<String, AgentSessionError> {
        if !self.is_available().await? {
            return Ok(
                "Oh My Pi executable is unavailable; check PATH or AIT_SERVER_OMP_BIN.".to_owned(),
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
    }

    async fn history(
        &self,
        handle: &AgentPersistenceHandle,
        cwd: &str,
    ) -> Result<Vec<crate::protocol::timeline::NativeItem>, AgentSessionError> {
        Ok(history::inspect(self, handle, cwd).await?.entries)
    }

    async fn inspect_session(
        &self,
        handle: &AgentPersistenceHandle,
        cwd: &str,
    ) -> Result<SessionHistory, AgentSessionError> {
        history::inspect(self, handle, cwd).await
    }

    async fn list_sessions(
        &self,
        options: &ListOptions,
    ) -> Result<Vec<SessionDescriptor>, AgentSessionError> {
        history::list(self, options).await
    }

    async fn create_session(
        &self,
        spec: &AgentSessionSpec,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        Ok(Box::new(bridge::Session::open(self, spec, None).await?) as Box<dyn AgentSession>)
    }

    async fn create_session_with_environment(
        &self,
        spec: &AgentSessionSpec,
        environment: &AgentEnvironment,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
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

        client.environment = AgentEnvironment::try_from(values)?;
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
        Ok(
            Box::new(bridge::Session::open(self, spec, Some(handle)).await?)
                as Box<dyn AgentSession>,
        )
    }
}

#[cfg(all(test, unix))]
mod tests;
