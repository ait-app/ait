//! Provider discovery, configuration and native history inspection.
mod native_sessions;
use std::{
    borrow::Cow,
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

#[cfg(test)]
use super::OpenCodeExecutionLimits;
use super::{
    Driver, live, projection, runtime, session,
    types::{DenyApprovals, Fault, Invocation, ProtocolError, Snapshot},
};
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

/// Installed `OpenCode` executable; authentication, model credentials and tools stay native.
#[derive(Clone, Debug)]
pub struct OpenCodeClient {
    driver: Driver,
}

impl OpenCodeClient {
    /// Execute the installed binary directly, without shell interpolation.
    #[must_use]
    pub fn new(program: PathBuf) -> Self {
        Self {
            driver: Driver::new(program),
        }
    }

    /// Bound newly generated native items, tokens and text before creating sessions.
    /// # Errors
    /// Rejects zero ceilings and transport sizes exceeding eight MiB.
    #[cfg(test)]
    pub fn with_execution_limits(
        mut self,
        limits: OpenCodeExecutionLimits,
    ) -> Result<Self, AgentSessionError> {
        self.driver = self.driver.with_execution_limits(limits).map_err(error)?;
        Ok(self)
    }

    async fn open(
        &self,
        spec: &AgentSessionSpec,
        binding: Option<(&AgentPersistenceHandle, AgentResumePurpose)>,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        validate_spec(spec)?;
        let mut config = spec.config.clone();
        let mut clients = BTreeMap::new();
        let id = if let Some((handle, _)) = binding {
            validate_handle(handle)?;
            let native = native_handle(handle)?;
            let mut saved: StoredAgentConfig = serde_json::from_value(native["config"].clone())
                .map_err(|_| AgentSessionError::Rejected)?;
            saved.mode_id.clone_from(&effective(&config, "").mode_id);
            if saved
                != effective(
                    &config,
                    native["model"]
                        .as_str()
                        .ok_or(AgentSessionError::Rejected)?,
                )
            {
                return Err(AgentSessionError::Rejected);
            }
            config.model = Some(
                native["model"]
                    .as_str()
                    .ok_or(AgentSessionError::Rejected)?
                    .to_owned(),
            );
            clients = serde_json::from_value(native["clients"].clone())
                .map_err(|_| AgentSessionError::Rejected)?;
            validate_clients(&clients)?;
            Some(handle.session_id.clone())
        } else {
            None
        };
        if config.model.is_none() {
            config.model = self
                .driver
                .discover_models(PathBuf::from(&spec.cwd))
                .await
                .map_err(error)?
                .first()
                .map(|model| model.id.clone());
        }
        let mut invocation = invocation(
            &AgentSessionSpec {
                config,
                ..spec.clone()
            },
            id,
        )?;
        if binding.is_some() {
            invocation.instructions = None;
        }
        let history_only =
            binding.is_some_and(|(_, purpose)| purpose == AgentResumePurpose::History);
        let connection = if history_only {
            invocation.verify_settings = false;
            let mut runtime = runtime::Runtime::spawn(
                &self.driver.binary,
                &invocation.cwd,
                &invocation.cancellation,
            )
            .await
            .map_err(error)?;
            let prepared = match session::snapshot(
                &runtime.api,
                invocation
                    .session_id
                    .as_deref()
                    .ok_or(AgentSessionError::Rejected)?,
                &invocation,
            )
            .await
            {
                Ok(value) => value,
                Err(err) => {
                    let _ = runtime.close().await;
                    return Err(error(err));
                }
            };
            session::Connection {
                runtime,
                invocation,
                prepared,
                submitted: false,
                limits: self.driver.limits,
            }
        } else {
            self.driver.open(invocation).await.map_err(error)?
        };
        Ok(Box::new(live::Session::new(
            connection,
            &spec.config,
            clients,
            history_only,
        )?))
    }

    async fn read(
        &self,
        handle: &AgentPersistenceHandle,
        cwd: &str,
    ) -> Result<SessionHistory, AgentSessionError> {
        validate_handle(handle)?;
        if handle.native_handle.is_none() {
            return self.read_external(handle, cwd).await;
        }
        let native = native_handle(handle)?;
        let config: StoredAgentConfig = serde_json::from_value(native["config"].clone())
            .map_err(|_| AgentSessionError::Rejected)?;
        let clients = serde_json::from_value(native["clients"].clone())
            .map_err(|_| AgentSessionError::Rejected)?;
        validate_clients(&clients)?;
        let mut request = invocation(
            &AgentSessionSpec {
                provider: "opencode".into(),
                cwd: cwd.into(),
                config: StoredAgentConfig {
                    model: Some(
                        native["model"]
                            .as_str()
                            .ok_or(AgentSessionError::Rejected)?
                            .into(),
                    ),
                    ..config.clone()
                },
            },
            Some(handle.session_id.clone()),
        )?;
        request.instructions = None;
        request.verify_settings = false;
        let mut runtime =
            runtime::Runtime::spawn(&self.driver.binary, &request.cwd, &request.cancellation)
                .await
                .map_err(error)?;
        let result = session::snapshot(&runtime.api, &handle.session_id, &request).await;
        let _ = runtime.close().await;
        let snapshot = result.map_err(error)?;
        history(&snapshot, config, &clients)
    }
}

impl AgentClient for OpenCodeClient {
    fn supports_metadata_generation(&self) -> bool {
        true
    }

    fn metadata_model(
        &self,
        models: &[Value],
    ) -> Option<::metadata::ports::generation::MetadataSelection> {
        crate::local::metadata_model::select(
            self.provider(),
            models,
            &["haiku", "mini", "flash", "minimax-m3", "nemotron-3-super"],
        )
    }

    fn generate_metadata<'a>(
        &'a self,
        spec: &'a AgentSessionSpec,
        prompt: &'a str,
        schema: &'a Value,
    ) -> AgentSessionFuture<'a, String> {
        Box::pin(super::metadata::generate(
            &self.driver.binary,
            spec,
            prompt,
            schema,
        ))
    }

    fn supports_session_import(&self) -> bool {
        true
    }
    fn list_sessions<'a>(
        &'a self,
        options: &'a ListOptions,
    ) -> AgentSessionFuture<'a, Vec<SessionDescriptor>> {
        Box::pin(self.list_native(options))
    }
    fn provider(&self) -> &'static str {
        "opencode"
    }
    fn validate_config(&self, config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        validate(config)
    }
    fn is_available(&self) -> AgentSessionFuture<'_, bool> {
        Box::pin(async move {
            let cwd = std::env::current_dir().map_err(|_| AgentSessionError::Unavailable)?;
            Ok(
                runtime::probe(&self.driver.binary, &cwd, &CancellationToken::new())
                    .await
                    .is_ok(),
            )
        })
    }
    fn settings(&self, _config: &StoredAgentConfig) -> Value {
        json!({"availableModes":modes(),"features":[],"capabilities":{"supportsStreaming":true,"supportsSessionListing":true,"supportsDynamicModes":false,"supportsMcpServers":false}})
    }
    fn discover<'a>(&'a self, cwd: &'a str) -> AgentSessionFuture<'a, Details> {
        Box::pin(async move {
            validate_directory(cwd)?;
            let models = self
                .driver
                .discover_models(cwd.into())
                .await
                .map_err(error)?;
            Ok(Details { models: models.iter().enumerate().map(|(index, model)| json!({"provider":"opencode","id":model.id,"label":model.name,"description":model.name,"isSelectable":true,"isDefault":index==0,
                "thinkingOptions":model.reasoning_efforts.iter().map(|id|json!({"id":id,"label":id})).collect::<Vec<_>>()})).collect(), modes: modes(), features: vec![] })
        })
    }
    fn create_session<'a>(
        &'a self,
        spec: &'a AgentSessionSpec,
    ) -> AgentSessionFuture<'a, Box<dyn AgentSession>> {
        Box::pin(self.open(spec, None))
    }
    fn resume_session<'a>(
        &'a self,
        handle: &'a AgentPersistenceHandle,
        spec: &'a AgentSessionSpec,
        purpose: AgentResumePurpose,
    ) -> AgentSessionFuture<'a, Box<dyn AgentSession>> {
        Box::pin(self.open(spec, Some((handle, purpose))))
    }
    fn history<'a>(
        &'a self,
        handle: &'a AgentPersistenceHandle,
        cwd: &'a str,
    ) -> AgentSessionFuture<'a, Vec<NativeItem>> {
        Box::pin(async move { Ok(self.read(handle, cwd).await?.entries) })
    }
    fn inspect_session<'a>(
        &'a self,
        handle: &'a AgentPersistenceHandle,
        cwd: &'a str,
    ) -> AgentSessionFuture<'a, SessionHistory> {
        Box::pin(self.read(handle, cwd))
    }
}

pub(super) fn error(error: ProtocolError) -> AgentSessionError {
    match error.code {
        Fault::AgentCapabilityUnsupported => AgentSessionError::Rejected,
        Fault::ProviderFailed
        | Fault::RunRecoveryFailed
        | Fault::SessionBusy
        | Fault::RunCancelled
        | Fault::RunLimitExceeded
        | Fault::ToolUseRequiresAssistant
        | Fault::ToolCallDuplicate => AgentSessionError::Failed,
    }
}

pub(super) fn validate(config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
    if config
        .mode_id
        .as_deref()
        .is_some_and(|id| !matches!(id, "build" | "plan"))
        || config.model.as_ref().is_some_and(|model| {
            model.len() > 512
                || model.chars().any(char::is_control)
                || model
                    .split_once('/')
                    .is_none_or(|(provider, model)| provider.is_empty() || model.is_empty())
        })
        || config
            .thinking_option_id
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 128 || id.chars().any(char::is_control))
        || config
            .feature_values
            .as_ref()
            .is_some_and(|map| !map.is_empty())
        || config
            .provider_options
            .as_ref()
            .is_some_and(|map| !map.is_empty())
        || config.tool_policy.is_some()
        || config.mcp_servers.is_some()
        || config
            .system_prompt
            .as_ref()
            .is_some_and(|text| text.len() > 65_536 || text.contains('\0'))
        || serde_json::to_vec(config)
            .map_err(|_| AgentSessionError::Rejected)?
            .len()
            > 65_536
    {
        return Err(AgentSessionError::Rejected);
    }
    Ok(())
}

pub(super) fn effective(config: &StoredAgentConfig, model: &str) -> StoredAgentConfig {
    StoredAgentConfig {
        mode_id: Some(config.mode_id.as_deref().unwrap_or("build").into()),
        model: Some(config.model.as_deref().unwrap_or(model).to_owned()),
        ..config.clone()
    }
}

fn validate_directory(cwd: &str) -> Result<(), AgentSessionError> {
    let path = Path::new(cwd);
    if !path.is_absolute()
        || !path.is_dir()
        || path
            .canonicalize()
            .map_err(|_| AgentSessionError::Rejected)?
            != path
    {
        return Err(AgentSessionError::Rejected);
    }
    Ok(())
}

fn validate_spec(spec: &AgentSessionSpec) -> Result<(), AgentSessionError> {
    if spec.provider != "opencode" {
        return Err(AgentSessionError::Rejected);
    }
    validate_directory(&spec.cwd)?;
    validate(&spec.config)
}

fn validate_handle(handle: &AgentPersistenceHandle) -> Result<(), AgentSessionError> {
    if handle.provider != "opencode" || !session::valid_id(&handle.session_id) {
        return Err(AgentSessionError::Rejected);
    }
    Ok(())
}

fn native_handle(handle: &AgentPersistenceHandle) -> Result<Cow<'_, Value>, AgentSessionError> {
    match handle.native_handle.as_ref() {
        Some(Value::String(encoded)) if encoded.len() <= 256 * 1024 => {
            let value: Value =
                serde_json::from_str(encoded).map_err(|_| AgentSessionError::Rejected)?;
            if !value.is_object() {
                return Err(AgentSessionError::Rejected);
            }
            Ok(Cow::Owned(value))
        }
        // Existing server records used an object before the frontend string contract was fixed.
        Some(value @ Value::Object(_)) => Ok(Cow::Borrowed(value)),
        None => handle
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("opencode"))
            .filter(|value| value.is_object())
            .map(Cow::Borrowed)
            .ok_or(AgentSessionError::Rejected),
        _ => Err(AgentSessionError::Rejected),
    }
}

fn validate_clients(clients: &BTreeMap<String, String>) -> Result<(), AgentSessionError> {
    if clients.len() > 512
        || clients.iter().any(|(native, client)| {
            !session::valid_id(native)
                || native.len() > 64
                || client.is_empty()
                || client.len() > 128
                || client.chars().any(char::is_control)
        })
    {
        return Err(AgentSessionError::Rejected);
    }
    Ok(())
}

fn invocation(
    spec: &AgentSessionSpec,
    session_id: Option<String>,
) -> Result<Invocation, AgentSessionError> {
    validate_spec(spec)?;
    Ok(Invocation {
        driver: "opencode".into(),
        request_id: uuid::Uuid::new_v4().to_string(),
        session_id,
        input_id: uuid::Uuid::new_v4().to_string(),
        prompt: String::new(),
        instructions: spec.config.system_prompt.clone(),
        cwd: spec.cwd.clone().into(),
        model: spec
            .config
            .model
            .clone()
            .ok_or(AgentSessionError::Rejected)?,
        reasoning_effort: spec.config.thinking_option_id.clone(),
        full_access: true,
        verify_settings: true,
        agent: spec.config.mode_id.as_deref().unwrap_or("build").into(),
        approvals: Arc::new(DenyApprovals),
        cancellation: CancellationToken::new(),
        cancel_acknowledged: Arc::default(),
    })
}

fn modes() -> Vec<Value> {
    vec![
        json!({"id":"build","label":"Build","description":"Use the native Build agent and OpenCode permission rules.","icon":"Hammer","colorTier":"moderate"}),
        json!({"id":"plan","label":"Plan","description":"Use the native Plan agent and its permission rules.","icon":"ShieldCheck","colorTier":"planning"}),
    ]
}

fn history(
    snapshot: &Snapshot,
    config: StoredAgentConfig,
    clients: &BTreeMap<String, String>,
) -> Result<SessionHistory, AgentSessionError> {
    let entries = projection::entries(snapshot, clients)?;
    let created_at = entries.first().map_or_else(
        || chrono::Utc::now().to_rfc3339(),
        |item| item.timestamp.clone(),
    );
    let prompts = entries
        .iter()
        .filter(|entry| entry.item["type"] == "user_message")
        .filter_map(|entry| entry.item["text"].as_str())
        .collect::<Vec<_>>();
    Ok(SessionHistory {
        resume_metadata: BTreeMap::new(),
        parent_id: None,
        descriptor: SessionDescriptor {
            provider_id: "opencode".into(),
            provider_label: "OpenCode".into(),
            provider_handle_id: snapshot.id.clone(),
            cwd: snapshot.cwd.to_string_lossy().into_owned(),
            title: None,
            first_prompt_preview: prompts.first().map(|text| (*text).to_owned()),
            last_prompt_preview: prompts.last().map(|text| (*text).to_owned()),
            last_activity_at: entries
                .last()
                .map_or_else(|| created_at.clone(), |item| item.timestamp.clone()),
        },
        created_at,
        config,
        active: false,
        entries,
    })
}

#[cfg(test)]
mod tests;
