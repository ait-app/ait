//! `DeepSeek` Harness native interactive Host, with explicit legacy ACP compatibility.

mod config;
mod launcher;
mod metadata;
mod native;
mod permissions;
mod session;
mod streaming;
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
use crate::protocol::provider::Details;

/// Stable provider identity used by creation, discovery and durable resume handles.
pub const PROVIDER: &str = "deepseek-harness";

/// Local Harness launcher; native profiles own credentials, tools and durable sessions.
#[derive(Debug, Clone)]
pub struct DeepSeekHarnessClient {
    program: PathBuf,
    desktop: Option<launcher::Desktop>,
    interactive: bool,
    deadline: Duration,
    environment: AgentEnvironment,
    images: super::images::ImageStore,
}

impl DeepSeekHarnessClient {
    /// Launch the native interactive Web Host without a shell, inheriting Harness configuration.
    /// Control operations have a thirty-second deadline; prompts have no duration limit.
    #[must_use]
    pub fn new(program: PathBuf) -> Self {
        Self {
            program,
            desktop: None,
            interactive: true,
            deadline: Duration::from_secs(30),
            environment: AgentEnvironment::default(),
            images: super::images::ImageStore::default(),
        }
    }

    /// Retain the automation-only ACP profile for older Harness installations.
    /// ACP does not expose permission presets or interactive user questions.
    #[must_use]
    pub fn with_acp_profile(mut self) -> Self {
        self.interactive = false;
        self
    }

    /// Materialize native image output in `directory` for live display and saved timelines.
    #[must_use]
    pub fn with_image_directory(mut self, directory: PathBuf) -> Self {
        self.images = super::images::ImageStore::new(directory);
        self
    }

    async fn probe(&self, spec: &AgentSessionSpec) -> Result<Details, AgentSessionError> {
        if self.interactive {
            let mut session = native::open(self, spec, None).await?;
            let details = session.details();
            let closed = session.close().await;
            let details = details?;
            closed?;
            return Ok(details);
        }
        let mut session = session::open(self, spec, None).await?;
        let details = config::details(&session.options);
        let closed = session.close().await;
        let details = details?;
        closed?;
        Ok(details)
    }
}

impl AgentClient for DeepSeekHarnessClient {
    fn supports_summary_generation(&self) -> bool {
        true
    }

    fn summary_model(&self, models: &[Value]) -> Option<model::summary::SummarySelection> {
        let mut selection = super::summary_model::select(
            self.provider(),
            models,
            &["deepseek-flash", "haiku", "mini", "flash"],
        )?;
        // The headless profile only exposes a reasoning override for the official
        // DeepSeek adapter. Other adapters retain their native defaults.
        let model: Vec<String> = serde_json::from_str(selection.model.as_deref()?).ok()?;
        if model
            .first()
            .is_none_or(|provider| provider != "deepseek-official")
        {
            selection.thinking_option_id = None;
        }
        Some(selection)
    }

    fn generate_summary<'a>(
        &'a self,
        spec: &'a AgentSessionSpec,
        prompt: &'a str,
        schema: &'a Value,
    ) -> AgentSessionFuture<'a, String> {
        Box::pin(self.metadata(spec, prompt, schema))
    }

    fn supports_session_import(&self) -> bool {
        self.interactive
    }

    fn list_sessions<'a>(
        &'a self,
        options: &'a crate::ports::native_history::ListOptions,
    ) -> AgentSessionFuture<'a, Vec<crate::ports::native_history::SessionDescriptor>> {
        Box::pin(native::native_sessions::list(self, options))
    }

    fn inspect_session<'a>(
        &'a self,
        handle: &'a AgentPersistenceHandle,
        cwd: &'a str,
    ) -> AgentSessionFuture<'a, crate::ports::native_history::SessionHistory> {
        Box::pin(native::native_sessions::inspect(self, handle, cwd))
    }

    fn provider(&self) -> &'static str {
        PROVIDER
    }

    fn supports_history_replay(&self) -> bool {
        self.interactive
    }

    fn history<'a>(
        &'a self,
        handle: &'a AgentPersistenceHandle,
        cwd: &'a str,
    ) -> AgentSessionFuture<'a, Vec<crate::protocol::timeline::NativeItem>> {
        Box::pin(async move {
            if !self.interactive {
                return Err(AgentSessionError::Unavailable);
            }
            native::history::read(self, handle, cwd).await
        })
    }

    fn validate_config(&self, config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        if self.interactive {
            native::validate(config)
        } else {
            config::validate(config)
        }
    }

    fn settings(&self, _config: &StoredAgentConfig) -> Value {
        if self.interactive {
            return json!({"availableModes":native::modes(),"features":[],"capabilities":{
                "supportsMcpServers":false,"supportsStreaming":true,"supportsReasoningStream":true,
                "supportsDynamicModes":true,"supportsSessionListing":true,
                "supportsRewindConversation":false,"supportsRewindFiles":false,"supportsRewindBoth":false}});
        }
        json!({"availableModes":[],"features":[],"capabilities":{
            "supportsMcpServers":true,"supportsStreaming":true,"supportsReasoningStream":true,
            "supportsDynamicModes":false,"supportsSessionListing":false,
            "supportsRewindConversation":false,"supportsRewindFiles":false,"supportsRewindBoth":false}})
    }

    fn is_available(&self) -> AgentSessionFuture<'_, bool> {
        Box::pin(async {
            Ok(super::configuration::executable(&self.program)
                && self
                    .desktop
                    .as_ref()
                    .is_none_or(|desktop| desktop.entrypoint.is_file()))
        })
    }

    fn diagnostic(&self) -> AgentSessionFuture<'_, String> {
        Box::pin(async move {
            if !self.is_available().await? {
                return Ok("DeepSeek Harness executable is unavailable".to_owned());
            }
            let cwd = std::env::current_dir().map_err(|_| AgentSessionError::Failed)?;
            let details = self
                .discover(cwd.to_str().ok_or(AgentSessionError::Failed)?)
                .await?;
            Ok(format!(
                "DeepSeek Harness {} ready; {} models",
                if self.interactive {
                    "native Host"
                } else {
                    "ACP v1"
                },
                details.models.len()
            ))
        })
    }

    fn discover<'a>(&'a self, cwd: &'a str) -> AgentSessionFuture<'a, Details> {
        Box::pin(async move {
            if self.interactive {
                return native::discover(self, cwd).await;
            }
            self.probe(&AgentSessionSpec {
                provider: PROVIDER.to_owned(),
                cwd: cwd.to_owned(),
                config: StoredAgentConfig::default(),
            })
            .await
        })
    }

    fn validate_selection<'a>(&'a self, spec: &'a AgentSessionSpec) -> AgentSessionFuture<'a, ()> {
        Box::pin(async move { self.probe(spec).await.map(|_| ()) })
    }

    fn create_session<'a>(
        &'a self,
        spec: &'a AgentSessionSpec,
    ) -> AgentSessionFuture<'a, Box<dyn AgentSession>> {
        Box::pin(async move {
            if self.interactive {
                Ok(Box::new(native::open(self, spec, None).await?) as Box<dyn AgentSession>)
            } else {
                Ok(Box::new(session::open(self, spec, None).await?) as Box<dyn AgentSession>)
            }
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
            if self.interactive {
                Ok(Box::new(native::open(self, spec, Some(handle)).await?)
                    as Box<dyn AgentSession>)
            } else {
                if handle
                    .metadata
                    .as_ref()
                    .is_some_and(|metadata| metadata.get("transport").is_some())
                {
                    return Err(AgentSessionError::Rejected);
                }
                Ok(Box::new(session::open(self, spec, Some(handle)).await?)
                    as Box<dyn AgentSession>)
            }
        })
    }
}

#[cfg(all(test, unix))]
mod tests;
