//! Tracked, bounded native push delivery independent of connected client subscriptions.

use std::sync::Arc;
use std::time::Duration;

use domain::agent_runtime::registry::AgentRuntimeRegistry;
use domain::session::protocol::SessionEventKind;
use futures_util::{StreamExt, stream};
use model::ErrorCode;
use serde_json::{Value, json};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use super::ports::{DeliveryError, Notification, PushTransport};
use crate::dispatch::State;

#[derive(Debug, Clone)]
pub(super) struct Dependencies {
    pub(super) transport: Arc<dyn PushTransport>,
    pub(super) agents: Arc<dyn AgentRuntimeRegistry>,
}

/// Start the installed delivery adapter once, before accepting client connections.
/// Tasks share runtime cancellation, blocking admission and shutdown tracking; no adapter is a no-op.
#[must_use]
pub fn start(state: &Arc<State>) -> Option<Worker> {
    let dependencies = state
        .push_tokens
        .as_ref()
        .and_then(|tokens| tokens.lock().ok()?.delivery.take());
    let dependencies = dependencies?;
    let events = state.session_events.subscribe_attention();
    let cancellation = state.cancellation.child_token();
    let worker = Worker(cancellation.clone());
    let state = state.clone();
    state.runtime.tasks.clone().spawn(async move {
        run(state, dependencies, events, cancellation).await;
    });
    Some(worker)
}

/// Owns the delivery task lifetime; dropping the API cancels HTTP work and its queue.
#[derive(Debug)]
pub struct Worker(CancellationToken);

impl Drop for Worker {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

async fn run(
    state: Arc<State>,
    dependencies: Dependencies,
    mut events: broadcast::Receiver<(SessionEventKind, Value)>,
    cancellation: CancellationToken,
) {
    loop {
        tokio::select! {
            biased;
            () = cancellation.cancelled() => break,
            event = events.recv() => match event {
                Ok((kind, payload)) => {
                    tokio::select! {
                        biased;
                        () = cancellation.cancelled() => break,
                        () = deliver(&state, &dependencies, kind, payload) => {},
                    }
                }
                Err(broadcast::error::RecvError::Lagged(count)) => {
                    tracing::warn!(count, "native push queue overflow; attention events dropped");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }
}

async fn deliver(
    state: &Arc<State>,
    dependencies: &Dependencies,
    kind: SessionEventKind,
    payload: Value,
) {
    let delivery = state
        .run_queued(state.push_tokens.clone(), ErrorCode::RegistryIo, {
            let server = state.info().server_id;
            let agents = dependencies.agents.clone();
            move |tokens| {
                let notification = prepare_notification(agents.as_ref(), kind, &payload, &server)?;
                let targets = if notification.is_some() {
                    tokens.active(chrono::Utc::now().timestamp_millis())
                } else {
                    Vec::new()
                };
                Ok((notification, targets))
            }
        })
        .await;
    let (notification, targets) = match delivery {
        Ok((Some(notification), targets)) => (notification, targets),
        Ok((None, _)) => return,
        Err(error) => {
            tracing::warn!(?error, "native push preparation failed");
            return;
        }
    };
    let foreground = state.session_events.foreground_push_tokens();
    let mut deliveries = stream::iter(
        targets
            .into_iter()
            .filter(|token| !foreground.contains(token)),
    )
    .map(|token| {
        let notification = &notification;
        async move {
            let result =
                send_with_retry(dependencies.transport.as_ref(), notification, &token).await;
            (token, result)
        }
    })
    .buffer_unordered(6);
    while let Some((token, result)) = deliveries.next().await {
        match result {
            Ok(()) => {}
            Err(DeliveryError::Unregistered) => {
                state.session_events.clear_push_token(&token);
                if state
                    .run_queued(
                        state.push_tokens.clone(),
                        ErrorCode::RegistryIo,
                        move |tokens| tokens.invalidate(&token).map_err(|_| ErrorCode::RegistryIo),
                    )
                    .await
                    .is_err()
                {
                    tracing::warn!("failed to persist invalid native push token removal");
                }
            }
            Err(error) => tracing::warn!(%error, "native push delivery failed"),
        }
    }
}

async fn send_with_retry(
    transport: &dyn PushTransport,
    notification: &Notification,
    token: &str,
) -> Result<(), DeliveryError> {
    for attempt in 0..3 {
        match transport.send(notification, token).await {
            Err(DeliveryError::Retryable(after)) if attempt < 2 => {
                tokio::time::sleep(Duration::from_secs((1 << attempt).max(after))).await;
            }
            result => return result,
        }
    }
    Err(DeliveryError::Retryable(0))
}

/// Prepare notification content from current Host state, without device or foreground filtering.
/// `agents` is blocking storage; callers must run this outside an async reactor.
/// Returns none for stale, cleared, internal, archived or unsupported attention.
/// # Errors
/// Returns `RegistryIo` when Agent state cannot be read.
pub fn prepare_notification(
    agents: &dyn AgentRuntimeRegistry,
    kind: SessionEventKind,
    payload: &Value,
    server: &str,
) -> Result<Option<Notification>, ErrorCode> {
    if kind == SessionEventKind::TerminalAttention {
        let (Some(id), Some(title), Some(body)) = (
            payload["terminalId"].as_str(),
            payload["title"].as_str(),
            payload["body"].as_str(),
        ) else {
            return Ok(None);
        };
        return Ok(Some(Notification {
            title: title.chars().take(120).collect(),
            body: body.chars().take(220).collect(),
            data: json!({"serverId":server,"terminalId":id,"workspaceId":payload["workspaceId"],"cwd":payload["cwd"]}),
        }));
    }
    if kind != SessionEventKind::AgentAttention {
        return Ok(None);
    }
    let Some(id) = payload["agentId"].as_str() else {
        return Ok(None);
    };
    let Some(record) = agents.get(id).map_err(|_| ErrorCode::RegistryIo)? else {
        return Ok(None);
    };
    if record.internal
        || record.archived_at.is_some()
        || !record.requires_attention
        || record.attention_timestamp.as_deref() != payload["timestamp"].as_str()
    {
        return Ok(None);
    }
    let Some(reason) = payload["reason"].as_str() else {
        return Ok(None);
    };
    let (title, fallback) = match reason {
        "finished" => ("Agent finished", "Finished working."),
        "permission" => ("Agent needs permission", "Permission requested."),
        "error" => ("Agent needs attention", "Encountered an error."),
        _ => return Ok(None),
    };
    Ok(Some(Notification {
        title: title.to_owned(),
        body: record
            .title
            .as_deref()
            .filter(|title| !title.trim().is_empty())
            .unwrap_or(fallback)
            .chars()
            .take(220)
            .collect(),
        data: json!({"serverId":server,"workspaceId":record.workspace_id,"agentId":id,"reason":reason}),
    }))
}

#[cfg(test)]
mod tests;
