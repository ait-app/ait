//! Pi's native JSONL RPC adapter. Authentication and sessions remain owned by Pi.

mod config;
mod history;
mod session;
mod streaming;
mod transport;

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};
use serde_json::{Value, json};

use crate::ports::agent_session::{
    AgentClient, AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionSpec,
};
use crate::ports::environment::AgentEnvironment;
use crate::protocol::provider::Details;

pub(crate) const PROVIDER: &str = "pi";

/// Native Pi executable and ephemeral environment; constructing it performs no I/O.
#[derive(Debug, Clone)]
pub(crate) struct PiClient {
    program: PathBuf,
    deadline: Duration,
    admission_deadline: Duration,
    version: Arc<transport::VersionProbe>,
    environment: AgentEnvironment,
    session_environment: history::SessionEnvironment,
    images: super::images::ImageStore,
}

impl PiClient {
    /// Select `program` without shell interpretation or executable fallback.
    /// Control operations have a ten-second deadline; prompt admission has a 120-second
    /// deadline. Inference after admission has no host duration limit.
    #[must_use]
    pub(crate) fn new(program: PathBuf) -> Self {
        Self {
            program,
            deadline: Duration::from_secs(10),
            admission_deadline: Duration::from_secs(120),
            version: Arc::default(),
            environment: AgentEnvironment::default(),
            session_environment: history::SessionEnvironment::default(),
            images: super::images::ImageStore::default(),
        }
    }

    /// Store submitted images in `directory` for durable timeline display.
    #[must_use]
    pub(crate) fn with_image_directory(mut self, directory: PathBuf) -> Self {
        self.images = super::images::ImageStore::new(directory);
        self
    }
}

#[async_trait::async_trait]
impl AgentClient for PiClient {
    fn supports_session_import(&self) -> bool {
        true
    }

    async fn list_sessions(
        &self,
        options: &crate::ports::native_history::ListOptions,
    ) -> Result<Vec<crate::ports::native_history::SessionDescriptor>, AgentSessionError> {
        let client = self.clone();
        let options = options.clone();

        tokio::task::spawn_blocking(move || history::list(&client, &options))
            .await
            .map_err(|error| history_task_failed("list_sessions", &error))?
    }

    async fn inspect_session(
        &self,
        handle: &AgentPersistenceHandle,
        cwd: &str,
    ) -> Result<crate::ports::native_history::SessionHistory, AgentSessionError> {
        let client = self.clone();
        let handle = handle.clone();
        let cwd = cwd.to_owned();

        tokio::task::spawn_blocking(move || history::inspect(&client, &handle, &cwd))
            .await
            .map_err(|error| history_task_failed("inspect_session", &error))?
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
        json!({"availableModes":[],"features":[],"capabilities":{
            "supportsStreaming":true,"supportsReasoningStream":true,
            "supportsMcpServers":false,"supportsDynamicModes":false,
            "supportsSessionListing":true,"supportsRewindConversation":false,
            "supportsRewindFiles":false,"supportsRewindBoth":false}})
    }

    async fn is_available(&self) -> Result<bool, AgentSessionError> {
        if !super::configuration::executable(&self.program) {
            return Ok(false);
        }
        Ok(transport::version(self).await.is_ok())
    }

    async fn diagnostic(&self) -> Result<String, AgentSessionError> {
        if !super::configuration::executable(&self.program) {
            return Ok("Pi executable is unavailable; check PATH or AIT_SERVER_PI_BIN.".to_owned());
        }
        match transport::version_info(self).await {
            Ok(version) if !version.supported => Ok(format!(
                "Pi {} is unsupported; requires a supported Pi version (1.2 or newer).",
                version.text
            )),
            Ok(_) => Ok(
                "Pi RPC ready (requires Pi 1.2 or newer); authentication is managed by Pi"
                    .to_owned(),
            ),
            Err(error) => Ok(error.to_string()),
        }
    }

    async fn discover(&self, cwd: &str) -> Result<Details, AgentSessionError> {
        config::directory(cwd)?;
        transport::version(self).await?;
        let mut transport = transport::Transport::spawn(self, cwd, &["--no-session".into()])?;
        let result = transport
            .request(json!({"type":"get_available_models"}))
            .await;
        transport.close().await?;
        Ok(Details {
            models: config::models(&result?)?,
            modes: Vec::new(),
            features: Vec::new(),
        })
    }

    async fn validate_selection(&self, spec: &AgentSessionSpec) -> Result<(), AgentSessionError> {
        config::spec(spec)?;
        if let Some(model) = &spec.config.model
            && !self
                .discover(&spec.cwd)
                .await?
                .models
                .iter()
                .any(|item| item["id"] == *model)
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
        client.version = Arc::default();
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
        let needs_lookup = handle.metadata.as_ref().is_some_and(|metadata| {
            metadata.get("hasInput") == Some(&Value::Bool(true))
                && handle
                    .native_handle
                    .as_ref()
                    .and_then(Value::as_str)
                    .or_else(|| metadata.get("sessionFile")?.as_str())
                    .is_none_or(|path| !std::path::Path::new(path).is_file())
        });
        let resolved = if needs_lookup {
            let history = self.inspect_session(handle, &spec.cwd).await?;
            let mut resolved = handle.clone();
            resolved.native_handle = None;
            resolved.metadata = Some(history.resume_metadata);
            Cow::Owned(resolved)
        } else {
            Cow::Borrowed(handle)
        };
        Ok(Box::new(session::open(self, spec, Some(&resolved)).await?) as Box<dyn AgentSession>)
    }
}

fn history_task_failed(
    operation: &'static str,
    error: &tokio::task::JoinError,
) -> AgentSessionError {
    tracing::error!(
        provider = PROVIDER,
        operation,
        error = %error,
        "Pi history task failed"
    );
    AgentSessionError::Failed
}

#[cfg(all(test, unix))]
mod tests;
