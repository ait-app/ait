//! Google Antigravity's official `agy` CLI over its headless NDJSON protocol.

mod config;
mod diagnostics;
mod discovery;
mod input;
mod quota;
mod session;
mod streaming;
mod summary;
mod transport;

use std::path::PathBuf;
use std::time::Duration;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};
use serde_json::{Value, json};

use provider_core::ports::agent_session::{
    AgentClient, AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionSpec,
};
use provider_core::ports::environment::AgentEnvironment;
use provider_core::protocol::provider::Details;

/// Stable provider identity, independent of the executable's `agy` name.
const PROVIDER: &str = "antigravity";

/// Installed native CLI; credentials, permissions and conversation storage remain AGY-owned.
#[derive(Debug, Clone)]
pub struct AntigravityClient {
    program: PathBuf,
    deadline: Duration,
    environment: AgentEnvironment,
    images: provider_native::images::ImageStore,
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
            images: provider_native::images::ImageStore::default(),
        }
    }

    /// Keep submitted images in the host's private artifact directory for native reads and history.
    #[must_use]
    pub fn with_image_directory(mut self, directory: PathBuf) -> Self {
        self.images = provider_native::images::ImageStore::new(directory);
        self
    }

    /// Find `agy` on PATH, then in official installer and Homebrew locations.
    ///
    /// Returns an unavailable launcher when no executable exists. Does not install software.
    #[must_use]
    pub fn installed() -> Self {
        Self::new(discovery::installed_program())
    }
}

#[async_trait::async_trait]
impl AgentClient for AntigravityClient {
    fn supports_write_confinement(&self) -> bool {
        true
    }

    fn provider(&self) -> &'static str {
        PROVIDER
    }

    fn catalog_presentation(&self) -> Option<provider_core::ports::catalog::CatalogPresentation> {
        Some(provider_core::ports::catalog::CatalogPresentation {
            label: "Antigravity",
            description: "Google Antigravity via the official AGY CLI",
            default_mode: provider_core::ports::catalog::DefaultMode::Fixed("default"),
        })
    }

    fn harness_evidence(
        &self,
    ) -> Option<provider_core::ports::harness_evidence::HarnessEvidenceSource> {
        Some(
            provider_core::ports::harness_evidence::HarnessEvidenceSource {
                program_variable: "AIT_SERVER_ANTIGRAVITY_BIN",
                default_program: "agy",
                log_directory_variable: "AIT_DIAGNOSTICS_ANTIGRAVITY_LOG_DIR",
                native_logs: provider_core::ports::harness_evidence::NativeLogs::UnderRoot {
                    root_variable: "HOME",
                    home_directory: "",
                    subdirectory: ".gemini/antigravity-cli/log",
                },
            },
        )
    }

    fn supports_summary_generation(&self) -> bool {
        true
    }

    fn summary_model(&self, models: &[Value]) -> Option<domain::summary::SummarySelection> {
        summary::select_model(models)
    }

    async fn generate_summary(
        &self,
        spec: &AgentSessionSpec,
        prompt: &str,
        schema: &Value,
    ) -> Result<String, AgentSessionError> {
        summary::generate(self, spec, prompt, schema).await
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

    async fn is_available(&self) -> Result<bool, AgentSessionError> {
        Ok(provider_native::configuration::executable(&self.program))
    }

    async fn diagnostic(&self) -> Result<String, AgentSessionError> {
        if !self.is_available().await? {
            return Ok("Antigravity CLI (agy) executable is unavailable".to_owned());
        }
        let cwd = std::env::current_dir().map_err(|_| AgentSessionError::Failed)?;
        let catalog =
            discovery::catalog(self, cwd.to_str().ok_or(AgentSessionError::Failed)?).await?;
        Ok(format!("Antigravity CLI ready; {} models", catalog.len()))
    }

    async fn usage(&self) -> Result<Value, AgentSessionError> {
        let cwd = std::env::current_dir().map_err(|_| AgentSessionError::Failed)?;
        quota::read(self, &cwd).await
    }

    async fn discover(&self, cwd: &str) -> Result<Details, AgentSessionError> {
        config::validate_directory(cwd)?;
        Ok(Details {
            models: discovery::catalog(self, cwd).await?.models(),
            modes: config::modes(),
            features: Vec::new(),
        })
    }

    async fn validate_selection(&self, spec: &AgentSessionSpec) -> Result<(), AgentSessionError> {
        config::validate_spec(spec)?;
        discovery::catalog(self, &spec.cwd)
            .await?
            .select(&spec.config)
            .map(drop)
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
