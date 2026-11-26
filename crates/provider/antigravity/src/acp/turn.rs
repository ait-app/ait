//! One owned worker reads a prompt and answers native callbacks without blocking host polling.

use std::{collections::BTreeMap, time::Duration};

use provider_core::ports::{
    agent_session::{AgentSessionError, AgentTurnEvent},
    diagnostics::{Category, Diagnostic},
};
use provider_native::{acp_permissions, acp_transport::Transport, images::ImageStore};
use serde_json::json;
use tokio::{sync::mpsc, task::JoinHandle};

use super::{projection::Projection, usage};

/// Bounded events and cancellation for the single worker that owns an admitted prompt.
#[derive(Debug)]
pub(super) struct Running {
    /// Events consumed by synchronous host polling or cancellation draining.
    pub(super) events: mpsc::Receiver<AgentTurnEvent>,
    /// Request native cancellation once; the prompt response remains authoritative.
    pub(super) cancel: mpsc::Sender<()>,
    task: Option<JoinHandle<Option<Transport>>>,
}

impl Running {
    /// Reclaim an idle transport after the terminal event, or recycle a failed connection.
    pub(super) async fn finish(mut self) -> Result<Option<Transport>, AgentSessionError> {
        self.task
            .take()
            .ok_or(AgentSessionError::Failed)?
            .await
            .map_err(|_| AgentSessionError::Failed)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

/// Transfer the admitted prompt to a bounded worker. Dropping its owner also kills the CLI group.
pub(super) fn spawn(
    transport: Transport,
    identity: (&str, &str, &str),
    images: ImageStore,
    completion_fixes: bool,
) -> Running {
    let (session, turn, rpc) = identity;
    let (send, events) = mpsc::channel(128);
    let (cancel, receive_cancel) = mpsc::channel(1);
    let session = session.to_owned();
    let turn = turn.to_owned();
    let rpc = rpc.to_owned();
    let task = tokio::spawn(async move {
        let mut transport = transport;
        let mut projection = Projection::new(&turn, images);
        projection.completion_fixes = completion_fixes;
        let result = receive(
            &mut transport,
            (&session, &turn, &rpc),
            &mut projection,
            &send,
            receive_cancel,
        )
        .await;
        let terminal = result.unwrap_or_else(|_| {
            if transport.diagnostics.failure().is_none() {
                transport
                    .diagnostics
                    .observe(Diagnostic::new(Category::Protocol, "acp"), "session/prompt");
            }
            AgentTurnEvent::Failed
        });
        projection.finish(terminal == AgentTurnEvent::Cancelled);
        let reusable = matches!(terminal, AgentTurnEvent::Completed(_));
        if !reusable {
            let _ = transport.close().await;
        }
        if drain(&mut projection, &send).await.is_err() || send.send(terminal).await.is_err() {
            return None;
        }
        reusable.then_some(transport)
    });
    Running {
        events,
        cancel,
        task: Some(task),
    }
}

async fn receive(
    transport: &mut Transport,
    identity: (&str, &str, &str),
    projection: &mut Projection,
    events: &mpsc::Sender<AgentTurnEvent>,
    mut cancel: mpsc::Receiver<()>,
) -> Result<AgentTurnEvent, AgentSessionError> {
    let (session, turn, rpc) = identity;
    let mut cancellation = None;
    let mut denied = false;
    loop {
        drain(projection, events).await?;
        let message = tokio::select! {
            _ = cancel.recv(), if cancellation.is_none() => {
                transport.send(&json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":session}})).await?;
                cancellation = Some(tokio::time::Instant::now() + Duration::from_secs(5));
                continue;
            }
            result = async {
                if let Some(deadline) = cancellation {
                    tokio::time::timeout_at(deadline,transport.receive()).await.unwrap_or_else(|_| {
                        transport.diagnostics.observe(Diagnostic::new(Category::Timeout,"acp"),"session/cancel");
                        Err(AgentSessionError::Failed)
                    })
                } else { transport.receive().await }
            } => result?,
        };
        match message["method"].as_str() {
            Some("session/update") => {
                if message["params"]["sessionId"] != session || message.get("id").is_some() {
                    return Err(AgentSessionError::Failed);
                }
                projection.update(&message["params"]["update"])?;
            }
            Some("session/request_permission") => {
                if message["params"]["sessionId"] != session {
                    return Err(AgentSessionError::Failed);
                }
                let pending = acp_permissions::capture(&message, crate::PROVIDER)?;
                denied = true;
                let response = acp_permissions::resolve(&pending, &json!({"behavior":"deny"}))
                    .unwrap_or_else(|_| acp_permissions::cancelled(&pending));
                transport.send(&response).await?;
            }
            Some(_) => {
                if let Some(id) = message.get("id") {
                    transport.send(&json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Unsupported client method"}})).await?;
                }
            }
            None if message["id"] == rpc => {
                let mut terminal = projection.stream.settle(
                    &message,
                    &mut BTreeMap::new(),
                    &transport.diagnostics,
                );
                if matches!(terminal, AgentTurnEvent::Completed(_))
                    && projection.has_running_tools()
                {
                    transport.diagnostics.observe_failure(
                        Diagnostic::new(Category::Protocol, "unfinished_tools"),
                        "AGY ended the turn while tools were still running. Completion could not be confirmed. Upgrade agy to 1.5.15 or newer if using an older release; the input will not be retried automatically.",
                        "session/prompt",
                    );
                    terminal = AgentTurnEvent::Failed;
                }
                if denied
                    && matches!(&terminal,AgentTurnEvent::Completed(text) if text.as_ref().is_none_or(|text|text.trim().is_empty()))
                {
                    transport.diagnostics.observe(
                        Diagnostic::new(Category::Permission, "permission"),
                        "session/prompt",
                    );
                    terminal = AgentTurnEvent::Failed;
                }
                if let Some(usage) = usage::request(turn, &message["result"]["usage"])? {
                    projection
                        .stream
                        .events
                        .push_back(AgentTurnEvent::RequestUsage(usage));
                }
                return Ok(terminal);
            }
            None => return Err(AgentSessionError::Failed),
        }
    }
}

async fn drain(
    projection: &mut Projection,
    events: &mpsc::Sender<AgentTurnEvent>,
) -> Result<(), AgentSessionError> {
    while let Some(event) = projection.stream.events.pop_front() {
        events
            .send(event)
            .await
            .map_err(|_| AgentSessionError::Failed)?;
    }
    Ok(())
}

/// Whether an event consumes the active turn exactly once.
pub(super) const fn terminal(event: &AgentTurnEvent) -> bool {
    matches!(
        event,
        AgentTurnEvent::Completed(_) | AgentTurnEvent::Cancelled | AgentTurnEvent::Failed
    )
}
