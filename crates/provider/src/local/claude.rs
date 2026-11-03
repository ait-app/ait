//! Native Claude Code sessions over the CLI's Agent SDK streaming control protocol.

mod config;
mod history;
mod inputs;
mod inspection;
mod metadata;
mod permissions;
mod rewind;
mod session;
mod streaming;
mod subagents;
mod synthetic;
mod tasks;
mod transport;

use std::path::PathBuf;
use std::time::Duration;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};
use serde_json::{Value, json};

use crate::ports::agent_session::{
    AgentClient, AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionSpec,
};
use crate::ports::native_history::{ListOptions, SessionDescriptor, SessionHistory};
use crate::protocol::{provider::Details, timeline::NativeItem};

/// Local Claude Code executable; credentials and native tools remain owned by Claude Code.
#[derive(Debug, Clone)]
pub(crate) struct ClaudeClient {
    program: PathBuf,
    config_dir: Option<PathBuf>,
    deadline: Duration,
    images: super::images::ImageStore,
    environment: crate::ports::environment::AgentEnvironment,
    resolved_models: std::sync::Arc<std::sync::RwLock<std::collections::BTreeMap<String, String>>>,
}

impl ClaudeClient {
    /// Use `program` without a shell and inherit Claude Code's configuration and authentication.
    /// Control requests time out after thirty seconds; foreground turns have no time limit.
    #[must_use]
    pub(crate) fn new(program: PathBuf) -> Self {
        Self {
            program,
            config_dir: std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from),
            deadline: Duration::from_secs(30),
            images: super::images::ImageStore::default(),
            environment: crate::ports::environment::AgentEnvironment::default(),
            resolved_models: std::sync::Arc::default(),
        }
    }

    /// Store decoded native image output in a private, persistent directory.
    #[must_use]
    pub(crate) fn with_image_directory(mut self, directory: PathBuf) -> Self {
        self.images = super::images::ImageStore::new(directory);
        self
    }

    async fn probe(&self, spec: &AgentSessionSpec) -> Result<Value, AgentSessionError> {
        config::validate_spec(spec)?;
        let mut transport = transport::Transport::spawn(self, spec, None, None)?;
        let result = transport.initialize().await;
        transport.close().await?;
        if let Ok(response) = &result {
            self.remember_models(response)?;
        }
        result
    }

    fn remember_models(&self, response: &Value) -> Result<(), AgentSessionError> {
        config::models(response)?;
        let mut models = self
            .resolved_models
            .write()
            .map_err(|_| AgentSessionError::Failed)?;
        models.clear();
        for model in response["models"].as_array().into_iter().flatten() {
            if let (Some(alias), Some(resolved)) = (
                model["value"].as_str(),
                model["resolvedModel"]
                    .as_str()
                    .filter(|value| value.len() <= 256),
            ) {
                models.insert(alias.to_owned(), resolved.to_owned());
            }
        }
        Ok(())
    }

    fn resolved_config(&self, config: &StoredAgentConfig) -> StoredAgentConfig {
        let mut resolved = config.clone();
        if let Some(model) = self.resolved_models.read().ok().and_then(|models| {
            models
                .get(config.model.as_deref().unwrap_or("default"))
                .cloned()
        }) {
            resolved.model = Some(model);
        }
        resolved
    }
}

#[async_trait::async_trait]
impl AgentClient for ClaudeClient {
    fn supports_agent_mcp(&self) -> bool {
        true
    }

    fn supports_summary_generation(&self) -> bool {
        true
    }

    fn supports_session_import(&self) -> bool {
        true
    }

    async fn create_session_with_environment(
        &self,
        spec: &AgentSessionSpec,
        environment: &crate::ports::environment::AgentEnvironment,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        let mut client = self.clone();
        client.environment = environment.clone();
        client.resolved_models = std::sync::Arc::default();
        if let Some((_, directory)) = environment
            .entries()
            .find(|(key, _)| *key == "CLAUDE_CONFIG_DIR")
        {
            client.config_dir = Some(PathBuf::from(directory));
        } else if client.config_dir.is_none()
            && let Some((_, home)) = environment.entries().find(|(key, _)| *key == "HOME")
        {
            client.config_dir = Some(PathBuf::from(home).join(".claude"));
        }
        session::open(&client, spec, None).await
    }

    fn summary_model(&self, models: &[Value]) -> Option<domain::summary::SummarySelection> {
        super::summary_model::select(self.provider(), models, &["haiku"])
    }

    async fn generate_summary(
        &self,
        spec: &AgentSessionSpec,
        prompt: &str,
        schema: &Value,
    ) -> Result<String, AgentSessionError> {
        self.metadata(spec, prompt, schema).await
    }

    fn provider(&self) -> &'static str {
        "claude"
    }

    fn validate_config(&self, config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        config::validate(&self.resolved_config(config))
    }

    async fn validate_selection(&self, spec: &AgentSessionSpec) -> Result<(), AgentSessionError> {
        config::validate_spec(spec)?;
        if self.validate_config(&spec.config).is_err() {
            self.probe(&AgentSessionSpec {
                config: StoredAgentConfig::default(),
                ..spec.clone()
            })
            .await?;
        }
        self.validate_config(&spec.config)
    }

    fn settings(&self, config: &StoredAgentConfig) -> Value {
        json!({"availableModes":config::modes(),"features":config::features(&self.resolved_config(config)),"capabilities":{
            "supportsMcpServers":true,
            "supportsDynamicModes":true,"supportsRewindConversation":true,"supportsRewindFiles":true,"supportsRewindBoth":true,
            "supportsStreaming":true,"supportsReasoningStream":true,"supportsSessionListing":true}})
    }

    async fn is_available(&self) -> Result<bool, AgentSessionError> {
        Ok(config::executable(&self.program))
    }

    async fn diagnostic(&self) -> Result<String, AgentSessionError> {
        self.native_diagnostic().await
    }

    async fn prepare_usage(
        &self,
    ) -> Result<crate::ports::account_usage::PreparedUsage<'_>, AgentSessionError> {
        self.prepare_native_usage().await
    }

    async fn usage(&self) -> Result<Value, AgentSessionError> {
        self.native_usage().await
    }

    async fn discover(&self, cwd: &str) -> Result<Details, AgentSessionError> {
        let response = self
            .probe(&AgentSessionSpec {
                provider: "claude".to_owned(),
                cwd: cwd.to_owned(),
                config: StoredAgentConfig::default(),
            })
            .await?;
        Ok(Details {
            models: config::models(&response)?,
            modes: config::modes(),
            features: config::features(&self.resolved_config(&StoredAgentConfig::default())),
        })
    }

    async fn commands(&self, spec: &AgentSessionSpec) -> Result<Vec<Value>, AgentSessionError> {
        let response = self.probe(spec).await?;
        config::commands(&response)
    }

    async fn create_session(
        &self,
        spec: &AgentSessionSpec,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        session::open(self, spec, None).await
    }

    async fn resume_session(
        &self,
        handle: &AgentPersistenceHandle,
        spec: &AgentSessionSpec,
        purpose: AgentResumePurpose,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        session::open(self, spec, Some((handle, purpose))).await
    }

    async fn history(
        &self,
        handle: &AgentPersistenceHandle,
        cwd: &str,
    ) -> Result<Vec<NativeItem>, AgentSessionError> {
        Ok(history::read(self, handle, cwd)?
            .ok_or(AgentSessionError::Unavailable)?
            .entries)
    }

    async fn inspect_session(
        &self,
        handle: &AgentPersistenceHandle,
        cwd: &str,
    ) -> Result<SessionHistory, AgentSessionError> {
        if handle
            .metadata
            .as_ref()
            .is_some_and(|metadata| metadata.contains_key(subagents::LOCATOR))
        {
            return subagents::read(self, handle, cwd);
        }
        history::read(self, handle, cwd)?.ok_or(AgentSessionError::Unavailable)
    }

    async fn subagents(
        &self,
        cwd: &str,
    ) -> Result<Vec<crate::ports::controls::NativeSubagent>, AgentSessionError> {
        subagents::list(self, cwd)
    }

    async fn rewind(
        &self,
        handle: &AgentPersistenceHandle,
        spec: &AgentSessionSpec,
        message: &str,
    ) -> Result<SessionHistory, AgentSessionError> {
        rewind::conversation(self, handle, spec, message)
    }

    async fn rewind_files(
        &self,
        handle: &AgentPersistenceHandle,
        spec: &AgentSessionSpec,
        message: &str,
    ) -> Result<(), AgentSessionError> {
        rewind::files(self, handle, spec, message).await
    }

    async fn list_sessions(
        &self,
        options: &ListOptions,
    ) -> Result<Vec<SessionDescriptor>, AgentSessionError> {
        history::list(self, options)
    }
}

#[cfg(test)]
mod tests;
