//! Oh My Pi provider: one unmodified `omp --mode rpc-ui` process per session, driven over
//! OMP's native JSONL RPC, with native events mapped directly onto Ait session events.
//!
//! OMP owns credentials, extensions, tools and session files. The daemon registers [`OmpClient`].

mod commands;
mod config;
mod diagnostic;
mod events;
mod framing;
mod history;
mod interactions;
mod prompt;
mod sandbox;
mod session;
mod stream;
mod subagents;
mod tool_detail;
mod transport;
mod usage;

use std::ffi::OsString;
use std::path::PathBuf;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};
use serde_json::{Value, json};

use provider::ports::agent_session::{
    AgentClient, AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionSpec,
};
use provider::ports::environment::AgentEnvironment;
use provider::ports::native_history::{ListOptions, SessionDescriptor, SessionHistory};
use provider::protocol::provider::Details;

/// Stable provider identity recorded on handles, descriptors and host requests.
const PROVIDER: &str = "omp";
/// Product name shown in descriptors and diagnostics.
const LABEL: &str = "Oh My Pi";

/// Installed OMP executable with bounded RPC startup and discovery.
#[derive(Debug, Clone)]
pub struct OmpClient {
    program: PathBuf,
    timeouts: transport::Timeouts,
    environment: AgentEnvironment,
    images: provider::local::images::ImageStore,
}

impl OmpClient {
    /// Use `program` directly, inheriting the local OMP configuration and authentication.
    #[must_use]
    pub fn new(program: PathBuf) -> Self {
        Self {
            program,
            timeouts: transport::Timeouts::default(),
            environment: AgentEnvironment::default(),
            images: provider::local::images::ImageStore::default(),
        }
    }

    /// Store rendered native image output in the daemon's private `directory`.
    #[must_use]
    pub fn with_image_directory(mut self, directory: PathBuf) -> Self {
        self.images = provider::local::images::ImageStore::new(directory);
        self
    }
}

/// Resolve the installed OMP executable through `environment`.
///
/// Returns `AIT_SERVER_OMP_BIN` when set, without checking that it exists, otherwise `omp`
/// for PATH lookup. Hosts pass `std::env::var_os`; tests pass an isolated lookup.
#[must_use]
pub fn installed_program(environment: impl Fn(&str) -> Option<OsString>) -> PathBuf {
    environment("AIT_SERVER_OMP_BIN").map_or_else(|| PathBuf::from("omp"), PathBuf::from)
}

#[async_trait::async_trait]
impl AgentClient for OmpClient {
    fn provider(&self) -> &'static str {
        PROVIDER
    }

    fn supports_write_confinement(&self) -> bool {
        true
    }

    fn supports_session_import(&self) -> bool {
        true
    }

    fn handles_out_of_band(&self, text: &str) -> bool {
        // Paseo's built-ins run beside foreground input; other slash text is a prompt (M5).
        commands::builtin(text).is_some()
    }

    fn validate_config(&self, config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        config::validate(config)
    }

    fn settings(&self, _config: &StoredAgentConfig) -> Value {
        // Fast is the only feature, and only a live session's native state can offer it. A mode
        // change relaunches OMP between turns (D23); MCP servers and rewind are not offered yet.
        json!({"availableModes":config::modes(),"features":[],"capabilities":{
            "supportsStreaming":true,"supportsReasoningStream":true,
            "supportsSessionPersistence":true,"supportsSessionListing":true,
            "supportsToolInvocations":true,"supportsMcpServers":false,
            "supportsDynamicModes":true,"supportsRewindConversation":false,
            "supportsRewindFiles":false,"supportsRewindBoth":false}})
    }

    async fn is_available(&self) -> Result<bool, AgentSessionError> {
        Ok(provider::local::configuration::executable(&self.program))
    }

    async fn discover(&self, cwd: &str) -> Result<Details, AgentSessionError> {
        let mut probe = session::probe(self, cwd, None, self.timeouts).await?;
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
        if !self.is_available().await? {
            return Err(AgentSessionError::Unavailable);
        }
        // As in Paseo, drafts offer no controls: Fast is read from a live session's state.
        Ok(Vec::new())
    }

    async fn commands(&self, spec: &AgentSessionSpec) -> Result<Vec<Value>, AgentSessionError> {
        config::validate_spec(spec)?;
        let timeouts = config::timeouts(&spec.config, self.timeouts);
        let mut probe =
            session::probe(self, &spec.cwd, spec.confinement.as_ref(), timeouts).await?;
        let result = probe.request("get_available_commands", json!({})).await;
        let closed = probe.close().await;
        let result = result?;
        closed?;
        commands::list(&result)
    }

    async fn diagnostic(&self) -> Result<String, AgentSessionError> {
        if !self.is_available().await? {
            return Ok(
                "Oh My Pi executable is unavailable; check PATH or AIT_SERVER_OMP_BIN.".to_owned(),
            );
        }
        // As in Paseo, no session process: `omp --version` and OMP's roots only (D31).
        Ok(diagnostic::report(self).await)
    }

    async fn history(
        &self,
        handle: &AgentPersistenceHandle,
        cwd: &str,
    ) -> Result<Vec<provider::protocol::timeline::NativeItem>, AgentSessionError> {
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

    async fn subagents(
        &self,
        cwd: &str,
    ) -> Result<Vec<provider::ports::controls::NativeSubagent>, AgentSessionError> {
        history::subagents(self, cwd).await
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
        Ok(Box::new(session::open(self, spec, Some(handle)).await?) as Box<dyn AgentSession>)
    }
}

#[cfg(all(test, unix))]
mod tests;
