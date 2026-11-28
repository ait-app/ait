//! Official ACP sessions. One owned asynchronous reader also answers permission callbacks.

mod config;
mod projection;
mod turn;
mod usage;
pub(super) mod version;

use std::{borrow::Cow, collections::VecDeque, path::Path, time::Duration};

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig, StoredAgentRuntimeInfo};
use provider_core::{
    ports::{
        agent_session::{AgentSession, AgentSessionError, AgentSessionSpec, AgentTurnEvent},
        diagnostics::{Category, Diagnostic},
    },
    protocol::{prompt::AgentPrompt, usage::AgentUsage},
};
use provider_native::{
    acp_config, acp_session, acp_transport::Transport, diagnostics::Diagnostics,
};
use serde_json::{Value, json};

use super::{AntigravityClient, PROVIDER, discovery};

/// Check the local ACP command without authentication, a conversation or a model turn.
pub(super) async fn available(client: &AntigravityClient, cwd: &Path) -> bool {
    super::transport::query(client, cwd, &["acp".as_ref(), "--help".as_ref()])
        .await
        .is_ok()
}

/// Startup failure with optional classified native diagnostic for reconnect reporting.
#[derive(Debug, thiserror::Error)]
pub(super) enum OpenError {
    #[error("AGY does not advertise the required ACP capability")]
    Unsupported,
    #[error(transparent)]
    Session(AgentSessionError),
    #[error("AGY ACP control request failed: {error}")]
    Diagnosed {
        error: AgentSessionError,
        diagnostic: Diagnostic,
    },
}

impl OpenError {
    /// Convert startup failures to the stable provider port error without retrying input.
    pub(super) const fn session(self) -> AgentSessionError {
        match self {
            Self::Unsupported => AgentSessionError::Rejected,
            Self::Session(error) | Self::Diagnosed { error, .. } => error,
        }
    }
}

impl From<AgentSessionError> for OpenError {
    fn from(error: AgentSessionError) -> Self {
        Self::Session(error)
    }
}

/// One native conversation over official ACP, with no history replay.
#[derive(Debug)]
pub(super) struct Session {
    client: AntigravityClient,
    spec: AgentSessionSpec,
    id: String,
    options: Value,
    capabilities: Value,
    usage: AgentUsage,
    context: (Option<u64>, Option<u64>),
    completion_fixes: bool,
    warned_resume: bool,
    transport: Option<Transport>,
    diagnostics: Diagnostics,
    running: Option<turn::Running>,
    active: Option<String>,
    events: VecDeque<AgentTurnEvent>,
    input_error: Option<&'static str>,
    closed: bool,
}

/// Create or resume the same native conversation after one authentication/model preflight.
/// The native handshake decides capability support; a version string never blocks admission.
pub(super) async fn open(
    client: &AntigravityClient,
    spec: &AgentSessionSpec,
    handle: Option<&AgentPersistenceHandle>,
) -> Result<Session, OpenError> {
    super::config::validate_spec(spec)?;
    if let Some(handle) = handle {
        validate_handle(handle, &spec.cwd)?;
    }
    let usage = usage::restore(handle)?;
    let selection = discovery::catalog(client, &spec.cwd)
        .await?
        .select(&spec.config)?;
    open_with_selection(client, spec, handle, usage, &selection).await
}

async fn open_with_selection(
    client: &AntigravityClient,
    spec: &AgentSessionSpec,
    handle: Option<&AgentPersistenceHandle>,
    usage: AgentUsage,
    selection: &discovery::Selection,
) -> Result<Session, OpenError> {
    let mut command = provider_native::sandbox::command(
        &client.program,
        spec.confinement.as_ref(),
        provider_native::sandbox::ANTIGRAVITY_STATE,
        &|name| client.environment.variable(name),
    )?;
    command
        .arg("acp")
        .envs(client.environment.entries())
        .env("AGY_CLI_DISABLE_AUTO_UPDATE", "true");
    if spec.config.mode_id.as_deref() == Some("full-access") {
        command.arg("--dangerously-skip-permissions");
    }
    if spec.confinement.is_some() {
        command.arg("--sandbox=false");
    }
    let mut transport = Transport::spawn(command, &spec.cwd, client.deadline, PROVIDER)?;
    let initialized = config::request(
        &mut transport,
        "initialize",
        json!({"protocolVersion":1,
        "clientInfo":{"name":"ait","version":env!("CARGO_PKG_VERSION")},"clientCapabilities":{}}),
    )
    .await?;
    if initialized["protocolVersion"] != 1 {
        return Err(AgentSessionError::Failed.into());
    }
    let capabilities = &initialized["agentCapabilities"];
    if handle.is_some() && !capabilities["sessionCapabilities"]["resume"].is_object() {
        return Err(OpenError::Unsupported);
    }
    // AGY connects HTTP servers on both openings without advertising `mcpCapabilities.http`.
    let mut params = json!({"cwd":spec.cwd,"mcpServers":acp_config::mcp_servers(&spec.config)});
    let method = if let Some(handle) = handle {
        params["sessionId"] = json!(handle.session_id);
        "session/resume"
    } else {
        "session/new"
    };
    let result = config::request(&mut transport, method, params).await?;
    let id = match handle {
        Some(handle) => {
            if result
                .get("sessionId")
                .is_some_and(|id| id != &handle.session_id)
            {
                return Err(AgentSessionError::Failed.into());
            }
            handle.session_id.clone()
        }
        None => acp_config::text(&result, "sessionId")?.to_owned(),
    };
    // The official CLI uses conversation UUIDs across sessions; never invent a mapping.
    uuid::Uuid::parse_str(&id).map_err(|_| AgentSessionError::Failed)?;
    let mut options = acp_config::state(&result)?;
    config::apply(
        &mut transport,
        &id,
        &mut options,
        selection,
        spec.config.mode_id.as_deref(),
    )
    .await?;
    Ok(Session {
        client: client.clone(),
        spec: spec.clone(),
        id,
        options,
        capabilities: capabilities.clone(),
        usage,
        context: (None, None),
        completion_fixes: version::classify(
            initialized["agentInfo"]["version"]
                .as_str()
                .unwrap_or_default()
                .as_bytes(),
        )
        .has_completion_fixes(),
        warned_resume: false,
        diagnostics: transport.diagnostics.clone(),
        transport: Some(transport),
        running: None,
        active: None,
        events: VecDeque::new(),
        input_error: None,
        closed: false,
    })
}

fn validate_handle(handle: &AgentPersistenceHandle, cwd: &str) -> Result<(), AgentSessionError> {
    if !acp_session::resumable(handle, PROVIDER, cwd)
        || uuid::Uuid::parse_str(&handle.session_id).is_err()
    {
        return Err(AgentSessionError::Rejected);
    }
    match handle
        .metadata
        .as_ref()
        .and_then(|meta| meta.get("transport"))
    {
        None => Ok(()),
        Some(value) if value == "acp" => Ok(()),
        Some(_) => Err(AgentSessionError::Rejected),
    }
}

impl Session {
    async fn reap(&mut self) -> Result<(), AgentSessionError> {
        if let Some(running) = self.running.take() {
            self.transport = running.finish().await?;
        }
        Ok(())
    }

    fn handle(&self) -> AgentPersistenceHandle {
        let mut handle = acp_session::persistence(PROVIDER, &self.id, &self.spec.cwd);
        if let Some(metadata) = &mut handle.metadata {
            metadata.insert("transport".to_owned(), json!("acp"));
            if self.usage != AgentUsage::default() {
                metadata.insert("acpUsage".to_owned(), json!(self.usage));
            }
        }
        handle
    }

    fn runtime(&self) -> StoredAgentRuntimeInfo {
        let value = |category| {
            acp_config::option(&self.options, category)
                .and_then(|option| option["currentValue"].as_str())
                .map(str::to_owned)
        };
        StoredAgentRuntimeInfo {
            provider: PROVIDER.to_owned(),
            session_id: Some(self.id.clone()),
            model: value("model"),
            thinking_option_id: value("thought_level"),
            mode_id: Some(
                self.spec
                    .config
                    .mode_id
                    .as_deref()
                    .unwrap_or("default")
                    .to_owned(),
            ),
            extra: None,
        }
    }

    async fn reconnect(&mut self, config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        if !self.capabilities["sessionCapabilities"]["resume"].is_object() {
            self.input_error = Some(
                "This AGY version does not support resuming this conversation or changing its configuration. Upgrade agy to 1.5.15 or newer. The previous configuration is retained; no input was sent.",
            );
            return Err(AgentSessionError::Rejected);
        }
        if let Some(mut transport) = self.transport.take() {
            transport.close().await?;
        }
        let spec = AgentSessionSpec {
            config: config.clone(),
            ..self.spec.clone()
        };
        match open(&self.client, &spec, Some(&self.handle())).await {
            Ok(mut replacement) => {
                replacement.events = std::mem::take(&mut self.events);
                replacement.context = self.context;
                *self = replacement;
                Ok(())
            }
            Err(error) => {
                if let OpenError::Diagnosed { diagnostic, .. } = &error {
                    self.diagnostics.observe(*diagnostic, "session/resume");
                } else if matches!(error, OpenError::Unsupported) {
                    self.input_error = Some(
                        "This AGY version does not support the requested model, thinking level, mode or conversation recovery over ACP. Upgrade agy to 1.5.15 or newer. The previous configuration is retained; no input was sent.",
                    );
                } else {
                    self.input_error = Some(
                        "AGY could not confirm the requested model, thinking level, mode or MCP servers on the original conversation. The previous configuration is retained; no input was sent.",
                    );
                }
                Err(error.session())
            }
        }
    }

    async fn start(
        &mut self,
        prompt: &AgentPrompt,
        config: &StoredAgentConfig,
    ) -> Result<String, AgentSessionError> {
        if self.closed || self.active.is_some() || self.events.iter().any(turn::terminal) {
            return Err(AgentSessionError::Rejected);
        }
        self.input_error = None;
        super::config::validate(config).inspect_err(|_| {
            self.input_error = Some("AGY does not support the requested session configuration.");
        })?;
        if prompt.output_schema.is_some() {
            self.input_error = Some(
                "AGY does not support JSON schema output constraints. Remove the output schema and retry.",
            );
            return Err(AgentSessionError::Rejected);
        }
        let blocks = prompt.blocks().inspect_err(|_| {
            self.input_error = Some("AGY input is empty, invalid or too large.");
        })?;
        if blocks.iter().any(|block| block["type"] == "image")
            && self.capabilities["promptCapabilities"]["image"] != true
        {
            self.input_error = Some(
                "This AGY version does not support image input over ACP. Upgrade agy to 1.5.15 or newer and reopen the session, or send text without images.",
            );
            return Err(AgentSessionError::Rejected);
        }
        let sources = blocks.iter().filter(|block| block["type"] == "image")
            .map(|block| self.client.images.source(block).map(Cow::into_owned))
            .collect::<Result<Vec<_>,_>>().inspect_err(|_| self.input_error = Some("Ait could not save AGY image attachments. Check disk space and provider image directory permissions."))?;
        self.reap().await?;
        if self.transport.is_none() || self.spec.config != *config {
            self.reconnect(config).await?;
        }
        self.diagnostics.clear();
        let mut transport = self.transport.take().ok_or(AgentSessionError::Failed)?;
        // Ownership stays here until admission is written exactly once. A failed write drops it.
        let rpc = transport
            .begin(
                "session/prompt",
                json!({"sessionId":self.id,"prompt":blocks}),
            )
            .await
            .inspect_err(|_| {
                if self.diagnostics.failure().is_none() {
                    self.diagnostics
                        .observe(Diagnostic::new(Category::Closed, "acp"), "session/prompt");
                }
            })?;
        let turn = uuid::Uuid::new_v4().to_string();
        self.events
            .push_back(AgentTurnEvent::RuntimeInfo(self.runtime()));
        self.events.extend(super::input::timeline(
            &self.id,
            &turn,
            &provider_native::prompt_blocks::text(&blocks),
            &sources,
            prompt.client_message_id.as_deref(),
        ));
        if !self.warned_resume && !self.capabilities["sessionCapabilities"]["resume"].is_object() {
            self.events.push_back(projection::notice(&turn, "resume", "当前 AGY 不支持恢复会话或在会话中切换配置；当前对话可继续使用。需要这些功能时，请升级 AGY 到 1.5.15 或更新版本。"));
            self.warned_resume = true;
        }
        self.running = Some(turn::spawn(
            transport,
            (&self.id, &turn, &rpc),
            self.client.images.clone(),
            self.completion_fixes,
        ));
        self.active = Some(turn.clone());
        Ok(turn)
    }

    fn observe(&mut self, event: AgentTurnEvent) -> AgentTurnEvent {
        if let AgentTurnEvent::RequestUsage(request) = &event {
            if usage::accumulate(&mut self.usage, request).is_err() {
                self.running.take();
                self.events.clear();
                self.active = None;
                self.diagnostics.observe(
                    Diagnostic::new(Category::Protocol, "usage"),
                    "session/prompt",
                );
                return AgentTurnEvent::Failed;
            }
            self.events
                .push_front(AgentTurnEvent::Usage(self.usage_snapshot()));
        }
        if let AgentTurnEvent::Usage(usage) = &event {
            if usage.context_window_used_tokens.is_some()
                || usage.context_window_max_tokens.is_some()
            {
                self.context = (
                    usage.context_window_used_tokens,
                    usage.context_window_max_tokens,
                );
            }
            return AgentTurnEvent::Usage(self.usage_snapshot());
        }
        if turn::terminal(&event) {
            self.active = None;
        }
        event
    }

    fn usage_snapshot(&self) -> AgentUsage {
        AgentUsage {
            context_window_used_tokens: self.context.0,
            context_window_max_tokens: self.context.1,
            ..self.usage.clone()
        }
    }
}

#[async_trait::async_trait]
impl AgentSession for Session {
    fn provider(&self) -> &'static str {
        PROVIDER
    }
    fn reports_token_usage(&self) -> bool {
        true
    }
    fn persistence(&self) -> Option<AgentPersistenceHandle> {
        Some(self.handle())
    }
    fn failure_message(&self) -> Option<Cow<'_, str>> {
        if let Some(message) = self.input_error {
            return Some(Cow::Borrowed(message));
        }
        self.diagnostics.message().map(Cow::Owned)
    }
    fn failure_diagnostic(&self) -> Option<Diagnostic> {
        if self.input_error.is_some() {
            return Some(Diagnostic::new(Category::Protocol, "input"));
        }
        self.diagnostics.failure()
    }
    async fn account_usage(&self) -> Result<Value, AgentSessionError> {
        super::quota::read(&self.client, Path::new(&self.spec.cwd)).await
    }
    async fn runtime_info(&mut self) -> Result<StoredAgentRuntimeInfo, AgentSessionError> {
        Ok(self.runtime())
    }
    async fn start_turn(
        &mut self,
        text: &str,
        config: &StoredAgentConfig,
    ) -> Result<String, AgentSessionError> {
        self.start(&AgentPrompt::text(text), config).await
    }
    async fn start_input(
        &mut self,
        prompt: &AgentPrompt,
        config: &StoredAgentConfig,
    ) -> Result<String, AgentSessionError> {
        self.start(prompt, config).await
    }
    fn poll_turn(&mut self) -> Result<Option<AgentTurnEvent>, AgentSessionError> {
        if let Some(event) = self.events.pop_front() {
            return Ok(Some(self.observe(event)));
        }
        let Some(running) = &mut self.running else {
            return Ok(None);
        };
        let event = match running.events.try_recv() {
            Ok(event) => Some(event),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => None,
            Err(_) if self.active.is_some() => Some(AgentTurnEvent::Failed),
            Err(_) => None,
        };
        Ok(event.map(|event| self.observe(event)))
    }
    async fn cancel_turn(&mut self, id: &str) -> Result<(), AgentSessionError> {
        if self.active.as_deref() != Some(id) || self.closed {
            return Err(AgentSessionError::Rejected);
        }
        if !self.completion_fixes {
            self.events.push_back(projection::notice(id, "cancel", "当前 AGY 版本可能缺少取消后的工具状态收尾；Ait 会结束仍显示运行中的工具卡片。升级 AGY 到 1.5.15 或更新版本可获得原生修复。"));
        }
        let running = self.running.as_mut().ok_or(AgentSessionError::Failed)?;
        let _ = running.cancel.try_send(());
        let result = tokio::time::timeout(self.client.deadline + Duration::from_secs(10), async {
            while let Some(event) = running.events.recv().await {
                let finished = turn::terminal(&event);
                let cancelled = event == AgentTurnEvent::Cancelled;
                self.events.push_back(event);
                if finished {
                    return Some(if cancelled {
                        Ok(())
                    } else {
                        Err(AgentSessionError::Rejected)
                    });
                }
            }
            None
        })
        .await;
        self.active = None;
        match result {
            Ok(None) | Err(_) => {
                self.running.take();
                self.events.push_back(AgentTurnEvent::Failed);
                Err(AgentSessionError::Failed)
            }
            Ok(Some(result)) => {
                self.reap().await?;
                result
            }
        }
    }
    async fn close(&mut self) -> Result<(), AgentSessionError> {
        if self.closed {
            return Ok(());
        }
        if let Some(id) = self.active.clone() {
            let _ = self.cancel_turn(&id).await;
        }
        self.closed = true;
        self.reap().await?;
        if let Some(mut transport) = self.transport.take() {
            acp_session::close(&mut transport, &self.id, &self.capabilities).await?;
        }
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests;
