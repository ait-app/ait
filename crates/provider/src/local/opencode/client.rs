//! `OpenCode` factory using official ACP and read-only native model discovery.
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};
use serde_json::{Value, json};

use super::{PROVIDER, config, history, launcher, session};
use crate::{
    ports::{
        agent_session::{
            AgentClient, AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionFuture,
            AgentSessionSpec,
        },
        native_history::{ListOptions, SessionDescriptor, SessionHistory},
    },
    protocol::{provider::Details, timeline::NativeItem},
};

/// Native `OpenCode` ACP subprocess. The installed executable owns authentication and history.
#[derive(Clone, Debug)]
pub struct OpenCodeClient {
    pub(super) program: PathBuf,
    pub(super) deadline: Duration,
    pub(super) environment: BTreeMap<String, String>,
    pub(super) images: crate::local::images::ImageStore,
}

impl OpenCodeClient {
    /// Launch `program acp` directly, inheriting native configuration and credentials.
    /// Control requests have a thirty-second deadline; model prompts have no duration limit.
    #[must_use]
    pub fn new(program: PathBuf) -> Self {
        Self {
            program,
            deadline: Duration::from_secs(30),
            environment: BTreeMap::new(),
            images: crate::local::images::ImageStore::default(),
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

impl AgentClient for OpenCodeClient {
    fn diagnostic(&self) -> AgentSessionFuture<'_, String> {
        Box::pin(async {
            let cwd = std::env::current_dir().map_err(|_| AgentSessionError::Unavailable)?;
            match launcher::version(self, &cwd.to_string_lossy()).await {
                Ok(version) => Ok(format!(
                    "OpenCode {version}: official ACP with native question forms."
                )),
                Err(_) => Ok(
                    "OpenCode ACP requires an installed OpenCode 2.x version 2.0.26 or newer."
                        .into(),
                ),
            }
        })
    }
    fn supports_summary_generation(&self) -> bool {
        true
    }
    fn generate_summary<'a>(
        &'a self,
        spec: &'a AgentSessionSpec,
        prompt: &'a str,
        schema: &'a Value,
    ) -> AgentSessionFuture<'a, String> {
        Box::pin(super::summary::generate(self, spec, prompt, schema))
    }
    fn summary_model(&self, models: &[Value]) -> Option<domain::summary::SummarySelection> {
        crate::local::summary_model::select(
            PROVIDER,
            models,
            &["haiku", "mini", "flash", "minimax-m3", "nemotron-3-super"],
        )
    }
    fn list_sessions<'a>(
        &'a self,
        options: &'a ListOptions,
    ) -> AgentSessionFuture<'a, Vec<SessionDescriptor>> {
        Box::pin(self.list(options))
    }
    fn provider(&self) -> &'static str {
        PROVIDER
    }
    fn supports_session_import(&self) -> bool {
        true
    }
    fn validate_config(&self, selected: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        config::validate(selected)
    }
    fn is_available(&self) -> AgentSessionFuture<'_, bool> {
        Box::pin(async {
            let cwd = std::env::current_dir().map_err(|_| AgentSessionError::Unavailable)?;
            match launcher::spawn(self, &cwd.to_string_lossy(), &StoredAgentConfig::default()).await
            {
                Ok((mut transport, _)) => {
                    transport.close().await?;
                    Ok(true)
                }
                Err(_) => Ok(false),
            }
        })
    }
    fn settings(&self, selected: &StoredAgentConfig) -> Value {
        json!({"availableModes":config::modes(),"features":config::features(selected),
            "capabilities":{"supportsStreaming":true,"supportsSessionListing":true,
                "supportsDynamicModes":true,"supportsMcpServers":false}})
    }
    fn discover<'a>(&'a self, cwd: &'a str) -> AgentSessionFuture<'a, Details> {
        Box::pin(async move {
            config::validate_spec(&AgentSessionSpec {
                provider: PROVIDER.into(),
                cwd: cwd.into(),
                config: StoredAgentConfig::default(),
            })?;
            super::discovery::discover(self, cwd).await
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
    fn resume_session<'a>(
        &'a self,
        handle: &'a AgentPersistenceHandle,
        spec: &'a AgentSessionSpec,
        purpose: AgentResumePurpose,
    ) -> AgentSessionFuture<'a, Box<dyn AgentSession>> {
        Box::pin(async move {
            Ok(
                Box::new(session::open(self, spec, Some((handle, purpose))).await?)
                    as Box<dyn AgentSession>,
            )
        })
    }
    fn history<'a>(
        &'a self,
        handle: &'a AgentPersistenceHandle,
        cwd: &'a str,
    ) -> AgentSessionFuture<'a, Vec<NativeItem>> {
        Box::pin(async move { Ok(history::inspect(self, handle, cwd).await?.entries) })
    }
    fn inspect_session<'a>(
        &'a self,
        handle: &'a AgentPersistenceHandle,
        cwd: &'a str,
    ) -> AgentSessionFuture<'a, SessionHistory> {
        Box::pin(history::inspect(self, handle, cwd))
    }
}
