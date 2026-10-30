//! `OpenCode` factory using official ACP and read-only native model discovery.
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, RwLock},
    time::Duration,
};

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};
use serde_json::{Value, json};

use super::{PROVIDER, config, history, launcher, session};
use crate::{
    ports::{
        agent_session::{
            AgentClient, AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionSpec,
        },
        native_history::{ListOptions, SessionDescriptor, SessionHistory},
    },
    protocol::{provider::Details, timeline::NativeItem},
};

/// Native `OpenCode` ACP subprocess. The installed executable owns authentication and history.
#[derive(Clone, Debug)]
pub(crate) struct OpenCodeClient {
    pub(super) program: PathBuf,
    pub(super) deadline: Duration,
    pub(super) startup_timeout: Duration,
    pub(super) environment: BTreeMap<String, String>,
    pub(super) images: crate::local::images::ImageStore,
    pub(super) capabilities: Arc<RwLock<Option<Capabilities>>>,
    pub(super) startup: Arc<tokio::sync::Mutex<()>>,
}

/// Session methods actually advertised by the installed native ACP process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Capabilities {
    pub(super) history: bool,
    pub(super) listing: bool,
    pub(super) deletion: bool,
}

impl OpenCodeClient {
    /// Launch `program acp` directly, inheriting native configuration and credentials.
    /// Ordinary controls use thirty seconds; startup and session creation have separate budgets.
    /// Model prompts have no duration limit.
    #[must_use]
    pub(crate) fn new(program: PathBuf) -> Self {
        Self {
            program,
            deadline: Duration::from_secs(30),
            startup_timeout: Duration::from_secs(120),
            environment: BTreeMap::new(),
            images: crate::local::images::ImageStore::default(),
            capabilities: Arc::new(RwLock::new(None)),
            startup: Arc::default(),
        }
    }

    async fn list(
        &self,
        options: &ListOptions,
    ) -> Result<Vec<SessionDescriptor>, AgentSessionError> {
        let cwd = options.cwd.clone().map_or_else(
            || {
                std::env::current_dir()
                    .map_err(|_| AgentSessionError::Failed)
                    .map(|cwd| cwd.to_string_lossy().into_owned())
            },
            Ok,
        )?;
        config::validate_spec(&AgentSessionSpec {
            provider: PROVIDER.into(),
            cwd: cwd.clone(),
            config: StoredAgentConfig::default(),
        })?;
        let (mut transport, capabilities) =
            launcher::spawn(self, &cwd, &StoredAgentConfig::default()).await?;
        if !capabilities["sessionCapabilities"]["list"].is_object() {
            return Err(AgentSessionError::Unavailable);
        }
        let result = history::list(&mut transport, options).await;
        transport.close().await?;
        let mut sessions = result?;
        history::populate(self, &mut sessions).await;
        Ok(sessions)
    }
}

#[async_trait::async_trait]
impl AgentClient for OpenCodeClient {
    async fn diagnostic(&self) -> Result<String, AgentSessionError> {
        let cwd = std::env::current_dir().map_err(|_| AgentSessionError::Unavailable)?;
        match launcher::version(self, &cwd.to_string_lossy()).await {
            Ok(version) if !launcher::supported(&version) => Ok(format!(
                "OpenCode {version} predates the supported ACP catalog and form implementation. \
                     Upgrade OpenCode to 2.0.25 or newer, then refresh the provider."
            )),
            Ok(version) => {
                match launcher::spawn(self, &cwd.to_string_lossy(), &StoredAgentConfig::default())
                    .await
                {
                    Ok((mut transport, _)) => {
                        transport.close().await?;
                        Ok(format!("OpenCode {version}: ACP initialized."))
                    }
                    Err(error) => Ok(format!("OpenCode {version}: ACP initialization {error}.")),
                }
            }
            Err(_) => Ok("OpenCode ACP requires an installed OpenCode 1.x or 2.x release.".into()),
        }
    }
    fn supports_summary_generation(&self) -> bool {
        true
    }
    async fn generate_summary(
        &self,
        spec: &AgentSessionSpec,
        prompt: &str,
        schema: &Value,
    ) -> Result<String, AgentSessionError> {
        super::summary::generate(self, spec, prompt, schema).await
    }
    fn summary_model(&self, models: &[Value]) -> Option<domain::summary::SummarySelection> {
        if !self.capabilities.read().ok()?.as_ref()?.deletion {
            return None;
        }
        crate::local::summary_model::select(
            PROVIDER,
            models,
            &["haiku", "mini", "flash", "minimax-m3", "nemotron-3-super"],
        )
    }
    async fn list_sessions(
        &self,
        options: &ListOptions,
    ) -> Result<Vec<SessionDescriptor>, AgentSessionError> {
        self.list(options).await
    }
    fn provider(&self) -> &'static str {
        PROVIDER
    }
    fn discovery_timeout(&self) -> Duration {
        // Include the executable probe, native initialization and session catalog inspection.
        self.startup_timeout * 2 + Duration::from_secs(60)
    }
    fn supports_session_import(&self) -> bool {
        self.capabilities
            .read()
            .ok()
            .and_then(|caps| *caps)
            .is_none_or(|caps| caps.history)
    }
    fn validate_config(&self, selected: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        config::validate(selected)
    }
    async fn is_available(&self) -> Result<bool, AgentSessionError> {
        let cwd = std::env::current_dir().map_err(|_| AgentSessionError::Unavailable)?;
        // Discovery validates ACP and records capabilities in the requested directory.
        // Probing it here would cold-start plugins twice, first in the daemon's directory.
        Ok(launcher::version(self, &cwd.to_string_lossy())
            .await
            .is_ok_and(|version| launcher::supported(&version)))
    }
    fn settings(&self, selected: &StoredAgentConfig) -> Value {
        let listing = self
            .capabilities
            .read()
            .ok()
            .and_then(|caps| *caps)
            .is_some_and(|caps| caps.listing);
        json!({"availableModes":[],"features":config::features(selected),
            "capabilities":{"supportsStreaming":true,"supportsSessionListing":listing,
                "supportsDynamicModes":true,"supportsMcpServers":false}})
    }
    async fn discover(&self, cwd: &str) -> Result<Details, AgentSessionError> {
        config::validate_spec(&AgentSessionSpec {
            provider: PROVIDER.into(),
            cwd: cwd.into(),
            config: StoredAgentConfig::default(),
        })?;
        super::discovery::discover(self, cwd).await
    }
    async fn create_session(
        &self,
        spec: &AgentSessionSpec,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        Ok(Box::new(session::open(self, spec, None).await?) as Box<dyn AgentSession>)
    }
    async fn resume_session(
        &self,
        handle: &AgentPersistenceHandle,
        spec: &AgentSessionSpec,
        purpose: AgentResumePurpose,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        Ok(
            Box::new(session::open(self, spec, Some((handle, purpose))).await?)
                as Box<dyn AgentSession>,
        )
    }
    async fn history(
        &self,
        handle: &AgentPersistenceHandle,
        cwd: &str,
    ) -> Result<Vec<NativeItem>, AgentSessionError> {
        Ok(history::inspect(self, handle, cwd).await?.entries)
    }
    async fn inspect_session(
        &self,
        handle: &AgentPersistenceHandle,
        cwd: &str,
    ) -> Result<SessionHistory, AgentSessionError> {
        history::inspect(self, handle, cwd).await
    }
}
