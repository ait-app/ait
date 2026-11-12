//! Pi's native JSONL RPC adapter. Authentication and sessions remain owned by Pi.

mod commands;
mod config;
mod history;
mod mcp;
mod probe;
mod session;
mod streaming;
mod transport;

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};
use serde_json::{Value, json};

use provider_core::ports::agent_session::{
    AgentClient, AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionSpec,
};
use provider_core::ports::environment::AgentEnvironment;
use provider_core::ports::native_history::CwdLookup;
use provider_core::protocol::provider::Details;

pub(crate) const PROVIDER: &str = "pi";
const DISCOVERY_DEFAULTS_TIMEOUT: Duration = Duration::from_secs(1);

/// Native Pi executable and ephemeral environment; constructing it performs no I/O.
#[derive(Debug, Clone)]
pub(crate) struct PiClient {
    program: PathBuf,
    deadline: Duration,
    admission_deadline: Duration,
    version: Arc<transport::VersionProbe>,
    environment: AgentEnvironment,
    session_environment: history::SessionEnvironment,
    images: provider_native::images::ImageStore,
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
            images: provider_native::images::ImageStore::default(),
        }
    }

    /// Store submitted images in `directory` for durable timeline display.
    #[must_use]
    pub(crate) fn with_image_directory(mut self, directory: PathBuf) -> Self {
        self.images = provider_native::images::ImageStore::new(directory);
        self
    }

    fn with_environment(&self, environment: &AgentEnvironment) -> Self {
        let mut client = self.clone();
        client.environment = environment.clone();
        client.version = Arc::default();
        client
    }

    /// Read the native catalog, optionally including defaults from the same process.
    /// Returns catalog errors; unavailable optional state leaves defaults unspecified.
    async fn discover_models(
        &self,
        cwd: &str,
        include_defaults: bool,
    ) -> Result<(Value, Value), AgentSessionError> {
        config::directory(cwd)?;
        probe::run(self, cwd, None, None, |transport| {
            Box::pin(async move {
                let models = transport
                    .request(json!({"type":"get_available_models"}))
                    .await?;
                // Capability discovery must remain usable when optional ambient defaults fail.
                let state = if include_defaults {
                    match tokio::time::timeout(
                        DISCOVERY_DEFAULTS_TIMEOUT,
                        transport.request(json!({"type":"get_state"})),
                    )
                    .await
                    {
                        Ok(Ok(state)) => state,
                        Ok(Err(_)) | Err(_) => Value::Null,
                    }
                } else {
                    Value::Null
                };
                Ok((models, state))
            })
        })
        .await
    }
}

#[async_trait::async_trait]
impl AgentClient for PiClient {
    fn supports_session_import(&self) -> bool {
        true
    }

    fn supports_write_confinement(&self) -> bool {
        true
    }

    /// Each interactive process loads a private extension that registers HTTP MCP servers
    /// with `pi.registerMcpServer` for that session only; `mcp.json` is never written.
    fn supports_agent_mcp(&self) -> bool {
        true
    }

    async fn list_sessions(
        &self,
        options: &provider_core::ports::native_history::ListOptions,
    ) -> Result<Vec<provider_core::ports::native_history::SessionDescriptor>, AgentSessionError>
    {
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
    ) -> Result<provider_core::ports::native_history::SessionHistory, AgentSessionError> {
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

    fn cwd_lookup(&self) -> CwdLookup {
        CwdLookup::Requested
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
        if !provider_native::configuration::executable(&self.program) {
            return Ok(false);
        }
        Ok(transport::version(self).await.is_ok())
    }

    async fn diagnostic(&self) -> Result<String, AgentSessionError> {
        if !provider_native::configuration::executable(&self.program) {
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
        let (models, state) = self.discover_models(cwd, true).await?;
        Ok(Details {
            models: config::models(&models, &state)?,
            modes: Vec::new(),
            features: Vec::new(),
        })
    }

    async fn commands(&self, spec: &AgentSessionSpec) -> Result<Vec<Value>, AgentSessionError> {
        if spec.provider != PROVIDER {
            return Err(AgentSessionError::Rejected);
        }
        if !std::path::Path::new(&spec.cwd).is_absolute() {
            return Err(AgentSessionError::Rejected);
        }
        commands::discover(self, &spec.cwd, spec.confinement.as_ref()).await
    }

    async fn validate_selection(&self, spec: &AgentSessionSpec) -> Result<(), AgentSessionError> {
        config::spec(spec)?;
        // Pi validates or clamps opaque levels against the selected model on creation,
        // configuration and resume. A stale catalog/profile must not prevent resume.
        if let Some(model) = &spec.config.model {
            let (models, _) = self.discover_models(&spec.cwd, false).await?;
            if !config::model_ids(&models)?.contains(model) {
                return Err(AgentSessionError::Rejected);
            }
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
        self.with_environment(environment)
            .create_session(spec)
            .await
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
        let required = handle
            .metadata
            .as_ref()
            .is_some_and(|metadata| metadata.get("hasInput") == Some(&Value::Bool(true)));
        let needs_lookup = handle.metadata.as_ref().is_some_and(|metadata| {
            (required || metadata.get("observedWork") == Some(&Value::Bool(true)))
                && handle
                    .native_handle
                    .as_ref()
                    .and_then(Value::as_str)
                    .or_else(|| metadata.get("sessionFile")?.as_str())
                    .is_none_or(|path| !std::path::Path::new(path).is_file())
        });
        let resolved = if needs_lookup {
            match self.inspect_session(handle, &spec.cwd).await {
                Ok(history) => {
                    let mut resolved = handle.clone();
                    resolved.native_handle = None;
                    resolved.metadata = Some(history.resume_metadata);
                    Cow::Owned(resolved)
                }
                // Observed work may never have been saved. A required file stays mandatory.
                Err(AgentSessionError::Unavailable) if !required => Cow::Borrowed(handle),
                Err(error) => return Err(error),
            }
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
