//! Ait-owned ACP client and protocol engine over one unmodified OMP RPC runtime.

mod aliases;
mod engine;
mod mapping;
mod protocol;

use std::borrow::Cow;
use std::collections::VecDeque;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};

use super::OmpClient;
use crate::ports::agent_session::{
    AgentSession, AgentSessionError, AgentSessionSpec, AgentTurnEvent,
};
use crate::ports::diagnostics::Diagnostic;
use crate::protocol::prompt::AgentPrompt;

/// Preserve native event fields while routing error payloads through bounded redaction.
/// These are Ait bridge extensions; no capability or extension is required from OMP.
pub(super) fn redact_event(event: &Value) -> Value {
    let safe_error = |error: &Value| {
        let mut failure = crate::local::diagnostics::failure::Failure::default();
        let diagnostic = crate::local::diagnostics::native(error, "omp_bridge");
        failure.observe("Oh My Pi", error, diagnostic);
        json!(failure.message().unwrap_or("OMP native error"))
    };
    let mut event = event.clone();
    if let Some(error) = event.get("error").filter(|error| !error.is_null()) {
        event["error"] = safe_error(error);
    }
    if event["message"]["stopReason"] == "error" {
        let error = safe_error(&event["message"]["errorMessage"]);
        event["message"] = json!({"role":"assistant","timestamp":event["message"]["timestamp"],
            "content":[],"stopReason":"error","errorMessage":error});
    }
    if event["assistantMessageEvent"]["type"] == "error" {
        event["assistantMessageEvent"] =
            json!({"type":"error","error":safe_error(&event["assistantMessageEvent"])});
    }
    event
}

/// ACP-facing host session; the same engine also serves external stdio clients.
#[derive(Debug)]
pub(super) struct Session {
    engine: engine::Engine,
    events: VecDeque<AgentTurnEvent>,
    sequence: u64,
    id: String,
}

impl Session {
    /// Initialize the Ait bridge and create/resume one official OMP runtime through ACP.
    /// Invalid configuration or native failures reject without replaying any user input.
    pub(super) async fn open(
        client: &OmpClient,
        spec: &AgentSessionSpec,
        handle: Option<&AgentPersistenceHandle>,
    ) -> Result<Self, AgentSessionError> {
        super::config::validate_spec(spec)?;
        let mut session = Self {
            engine: engine::Engine::new(client.clone()),
            events: VecDeque::new(),
            sequence: 0,
            id: String::new(),
        };
        session
            .request(
                "initialize",
                json!({"protocolVersion":1,
            "clientCapabilities":{"elicitation":{"form":{}},"_meta":{"ait":1}},
            "clientInfo":{"name":"ait","version":env!("CARGO_PKG_VERSION")}}),
            )
            .await?;
        let mut params = json!({"cwd":spec.cwd,"mcpServers":[],
            "_meta":{"ait":{"config":spec.config,"persistence":handle}}});
        let method = if let Some(handle) = handle {
            params["sessionId"] = json!(handle.session_id);
            "session/resume"
        } else {
            "session/new"
        };
        let result = session.request(method, params).await?;
        super::config::text(&result, "sessionId")?.clone_into(&mut session.id);
        Ok(session)
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, AgentSessionError> {
        self.sequence += 1;
        let id = json!(self.sequence);
        let messages = self
            .engine
            .dispatch(json!({"jsonrpc":"2.0","id":id,
            "method":method,"params":params}))
            .await;
        let mut result = None;
        for message in messages {
            if message.get("method").is_none() && message["id"] == id {
                result = Some(crate::local::acp_transport::response(&message));
            } else {
                self.consume(&message)?;
            }
        }
        result.ok_or(AgentSessionError::Failed)?
    }

    fn consume(&mut self, message: &Value) -> Result<(), AgentSessionError> {
        let event = if message["method"] == "_ait/event" {
            message["params"].get("event")
        } else if message.get("method").is_none() {
            message["result"]["_meta"]["ait"]
                .get("event")
                .or_else(|| message["error"]["data"]["_meta"]["ait"].get("event"))
        } else {
            None
        };
        if let Some(event) = event {
            self.events.push_back(
                serde_json::from_value(event.clone()).map_err(|_| AgentSessionError::Failed)?,
            );
        }
        Ok(())
    }

    async fn start(
        &mut self,
        prompt: &AgentPrompt,
        config: &StoredAgentConfig,
    ) -> Result<String, AgentSessionError> {
        prompt.validate()?;
        self.sequence += 1;
        let messages = self
            .engine
            .dispatch(json!({"jsonrpc":"2.0","id":self.sequence,
            "method":"session/prompt","params":{"sessionId":self.id,
            "prompt":prompt.blocks()?,"_meta":{"ait":{"config":config,"prompt":prompt}}}}))
            .await;
        let mut turn = None;
        for message in messages {
            if message["method"] == "_ait/prompt_accepted" {
                turn = Some(super::config::text(&message["params"], "turnId")?.to_owned());
            } else if message.get("error").is_some() {
                return Err(AgentSessionError::Rejected);
            } else {
                self.consume(&message)?;
            }
        }
        turn.ok_or(AgentSessionError::Failed)
    }
}

#[async_trait::async_trait]
impl AgentSession for Session {
    fn provider(&self) -> &'static str {
        "omp"
    }
    fn control_settings(&self, config: &StoredAgentConfig) -> Option<Value> {
        self.engine
            .native()
            .and_then(|session| session.control_settings(config))
    }
    fn validate_config_update(&self, config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        super::config::validate(config)
    }
    fn failure_diagnostic(&self) -> Option<Diagnostic> {
        self.engine
            .native()
            .and_then(AgentSession::failure_diagnostic)
    }
    fn failure_message(&self) -> Option<Cow<'_, str>> {
        self.engine.native().and_then(AgentSession::failure_message)
    }
    async fn runtime_info(&mut self) -> Result<StoredAgentRuntimeInfo, AgentSessionError> {
        serde_json::from_value(
            self.request("_ait/runtime", json!({"sessionId":self.id}))
                .await?,
        )
        .map_err(|_| AgentSessionError::Failed)
    }
    fn persistence(&self) -> Option<AgentPersistenceHandle> {
        self.engine
            .native()
            .and_then(AgentSession::persistence)
            .map(|mut handle| {
                let metadata = handle.metadata.get_or_insert_default();
                metadata.insert("transport".to_owned(), json!("acp"));
                metadata.insert("nativeTransport".to_owned(), json!("rpc"));
                handle
            })
    }
    fn subagents(&self) -> Vec<crate::ports::controls::NativeSubagent> {
        self.engine
            .native()
            .map(AgentSession::subagents)
            .unwrap_or_default()
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
    async fn steer_turn(&mut self, turn: &str, text: &str) -> Result<(), AgentSessionError> {
        self.steer_input(turn, &AgentPrompt::text(text)).await
    }
    async fn steer_input(
        &mut self,
        turn: &str,
        prompt: &AgentPrompt,
    ) -> Result<(), AgentSessionError> {
        self.request(
            "_session/steering",
            json!({"sessionId":self.id,"prompt":prompt.blocks()?,
                "_meta":{"ait":{"turnId":turn,"prompt":prompt}}}),
        )
        .await
        .map(|_| ())
    }
    async fn cancel_turn(&mut self, turn: &str) -> Result<(), AgentSessionError> {
        self.request(
            "session/cancel",
            json!({"sessionId":self.id,"_meta":{"ait":{"turnId":turn}}}),
        )
        .await
        .map(|_| ())
    }
    fn pending_permissions(&self) -> Vec<Value> {
        self.engine
            .native()
            .map(AgentSession::pending_permissions)
            .unwrap_or_default()
    }
    async fn respond_permission(
        &mut self,
        id: &str,
        response: &Value,
    ) -> Result<(), AgentSessionError> {
        self.request(
            "_ait/respond_permission",
            json!({"sessionId":self.id,"id":id,"response":response}),
        )
        .await
        .map(|_| ())
    }
    fn poll_turn(&mut self) -> Result<Option<AgentTurnEvent>, AgentSessionError> {
        if let Some(event) = self.events.pop_front() {
            return Ok(Some(event));
        }
        for message in self.engine.poll()? {
            self.consume(&message)?;
        }
        Ok(self.events.pop_front())
    }
    async fn close(&mut self) -> Result<(), AgentSessionError> {
        self.request("session/close", json!({"sessionId":self.id}))
            .await
            .map(|_| ())
    }
}

/// Run the bridge on a bounded JSONL stream, sharing the exact host protocol engine.
/// Returns I/O/protocol failures; EOF closes the owned native process without deleting history.
pub(crate) async fn serve<R, W>(client: OmpClient, input: R, output: W) -> std::io::Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    protocol::serve(engine::Engine::new(client), input, output).await
}

#[cfg(all(test, unix))]
mod tests;
