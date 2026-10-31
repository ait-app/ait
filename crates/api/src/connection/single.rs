//! Four bounded capability workers behind one physical, connection-owned socket.
use std::time::Duration;

use axum::extract::ws::WebSocket;
use futures_util::{
    StreamExt,
    future::BoxFuture,
    stream::{FuturesUnordered, SplitStream},
};
use model::outbound::{Outbound, QueueError};
use model::server::SubscriptionReleaseRequest;
use model::server::{ClientMessage, ErrorCode, valid_id};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use super::{ConnectionSubscriptions, Incoming, Peer, chunks, error, process_message};
use crate::Shared;

// Workspace bootstrap sends several reads per group before the first response arrives.
const WORKER_QUEUE_CAPACITY: usize = 16;

enum Work {
    Message(Incoming),
    Release(String, oneshot::Sender<()>),
}

pub(super) async fn read(
    stream: SplitStream<WebSocket>,
    state: &Shared,
    lanes: &[Outbound; 4],
    peer: Peer<'_>,
    mut subscriptions: ConnectionSubscriptions,
) -> Result<(), QueueError> {
    let provider = ConnectionSubscriptions {
        provider: std::mem::take(&mut subscriptions.provider),
        ..Default::default()
    };
    let (metadata_tx, metadata_rx) = mpsc::channel(WORKER_QUEUE_CAPACITY);
    let (terminal_tx, terminal_rx) = mpsc::channel(WORKER_QUEUE_CAPACITY);
    let (files_tx, files_rx) = mpsc::channel(WORKER_QUEUE_CAPACITY);
    let (provider_tx, provider_rx) = mpsc::channel(WORKER_QUEUE_CAPACITY);
    let cancel = CancellationToken::new();
    let contexts = lanes.each_ref().map(|outbound| WorkerContext {
        state,
        outbound,
        peer,
        cancel: &cancel,
    });
    let reader = async {
        let result = route(
            stream,
            &contexts[0],
            [metadata_tx, terminal_tx, files_tx, provider_tx],
        )
        .await;
        cancel.cancel();
        result
    };
    tokio::try_join!(
        reader,
        worker(metadata_rx, subscriptions, &contexts[0]),
        worker(
            terminal_rx,
            ConnectionSubscriptions::default(),
            &contexts[1]
        ),
        worker(files_rx, ConnectionSubscriptions::default(), &contexts[2]),
        worker(provider_rx, provider, &contexts[3])
    )?;
    Ok(())
}

struct WorkerContext<'a> {
    state: &'a Shared,
    outbound: &'a Outbound,
    peer: Peer<'a>,
    cancel: &'a CancellationToken,
}

async fn worker(
    mut receiver: mpsc::Receiver<Work>,
    mut subscriptions: ConnectionSubscriptions,
    context: &WorkerContext<'_>,
) -> Result<(), QueueError> {
    let mut poll = tokio::time::interval(Duration::from_millis(40));
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut expiry = tokio::time::interval(Duration::from_secs(30));
    loop {
        let work = tokio::select! {
            biased;
            () = context.cancel.cancelled() => return Ok(()),
            _ = poll.tick(), if !subscriptions.terminals.is_empty() || !subscriptions.voice.is_empty() => {
                subscriptions.terminals.poll(&context.state.terminal, context.outbound).await?;
                if let Some(speech) = &context.state.voice.speech {
                    subscriptions.voice.poll(speech, &context.state.runtime, context.outbound)?;
                }
                continue;
            },
            work = receiver.recv() => match work { Some(work) => work, None => return Ok(()) },
            _ = expiry.tick() => { subscriptions.filesystem.files.prune_uploads(); continue; },
        };
        match work {
            Work::Release(id, done) => {
                subscriptions.release(&id);
                let _ = done.send(());
            }
            Work::Message(message) => {
                let result = tokio::select! {
                    () = context.cancel.cancelled() => return Ok(()),
                    result = process_message(Some(Ok(message)), context.state, context.outbound,
                        context.peer, &mut subscriptions) => result?,
                };
                if result.is_break() {
                    context.cancel.cancel();
                    return Ok(());
                }
            }
        }
    }
}

async fn route(
    mut stream: SplitStream<WebSocket>,
    context: &WorkerContext<'_>,
    senders: [mpsc::Sender<Work>; 4],
) -> Result<(), QueueError> {
    let mut assembly = chunks::Assembly::default();
    let mut releases: FuturesUnordered<BoxFuture<'static, Result<(), QueueError>>> =
        FuturesUnordered::new();
    loop {
        let message = tokio::select! {
            () = context.cancel.cancelled() => return Ok(()),
            () = context.state.cancellation.cancelled() => return Ok(()),
            Some(result) = releases.next() => { result?; continue; },
            message = chunks::next(&mut stream, &mut assembly, context.outbound) => match message {
                Some(Ok(message)) => message,
                Some(Err(code)) => { error(context.outbound, None, code)?; return Ok(()); },
                None => return Ok(()),
            },
        };
        if let Incoming::Text(
            ClientMessage::Request {
                request_id,
                method,
                params,
            },
            _,
        ) = &message
        {
            if !valid_id(request_id) {
                return error(context.outbound, None, ErrorCode::InvalidMessage);
            }
            if let Err(code) = validate_request(method, context) {
                reject_negotiation(context, request_id, method, code)?;
                continue;
            }
            if method == "connection.ping" {
                context.outbound.respond(
                    request_id.clone(),
                    metadata::server::rpc::server::ping(params.clone())
                        .map_err(|_| ErrorCode::InvalidMessage),
                )?;
                continue;
            }
            if method == "subscription.release.request" {
                let Ok(release) =
                    serde_json::from_value::<SubscriptionReleaseRequest>(params.clone())
                else {
                    error(
                        context.outbound,
                        Some(request_id.clone()),
                        ErrorCode::InvalidMessage,
                    )?;
                    continue;
                };
                if !valid_id(&release.subscription_id) {
                    error(
                        context.outbound,
                        Some(request_id.clone()),
                        ErrorCode::InvalidMessage,
                    )?;
                    continue;
                }
                if releases.len() >= 32 {
                    error(
                        context.outbound,
                        Some(request_id.clone()),
                        ErrorCode::ResourceExhausted,
                    )?;
                    continue;
                }
                let slots: Result<Vec<_>, _> =
                    senders.iter().map(mpsc::Sender::try_reserve).collect();
                let Ok(slots) = slots else {
                    error(
                        context.outbound,
                        Some(request_id.clone()),
                        ErrorCode::ResourceExhausted,
                    )?;
                    continue;
                };
                let mut done = Vec::with_capacity(4);
                for slot in slots {
                    let (tx, rx) = oneshot::channel();
                    slot.send(Work::Release(release.subscription_id.clone(), tx));
                    done.push(rx);
                }
                let outbound = context.outbound.clone();
                let request_id = request_id.clone();
                releases.push(Box::pin(async move {
                    for receiver in done {
                        receiver.await.map_err(|_| QueueError::Full)?;
                    }
                    outbound.respond(
                        request_id,
                        Ok(serde_json::json!({"subscriptionId":release.subscription_id})),
                    )
                }));
                continue;
            }
        }
        admit(&senders, message, context.outbound)?;
    }
}

fn validate_request(method: &str, context: &WorkerContext<'_>) -> Result<(), ErrorCode> {
    super::validation::request(
        method,
        &context.state.info.implemented_capabilities,
        context.peer.capabilities,
    )
}

fn reject_negotiation(
    context: &WorkerContext<'_>,
    request_id: &str,
    method: &str,
    code: ErrorCode,
) -> Result<(), QueueError> {
    tracing::info_span!(
        "rpc",
        method,
        request_id,
        resource = "capability_negotiation",
        installed = context
            .state
            .info
            .capabilities
            .iter()
            .any(|name| name == method),
        negotiated = context.peer.capabilities.iter().any(|name| name == method),
    )
    .in_scope(|| error(context.outbound, Some(request_id.to_owned()), code))
}

fn admit(
    senders: &[mpsc::Sender<Work>; 4],
    message: Incoming,
    outbound: &Outbound,
) -> Result<(), QueueError> {
    let lane = lane(&message);
    if let Err(failure) = senders[lane].try_send(Work::Message(message)) {
        if let Work::Message(Incoming::Text(
            ClientMessage::Request {
                request_id, method, ..
            },
            _,
        )) = failure.into_inner()
        {
            tracing::info_span!(
                "rpc",
                method = %method,
                request_id = %request_id,
                capability_lane = lane,
                resource = "capability_queue",
                queue_capacity = WORKER_QUEUE_CAPACITY,
            )
            .in_scope(|| error(outbound, Some(request_id), ErrorCode::ResourceExhausted))?;
        } else {
            // Binary/event loss cannot be repaired by retrying an RPC.
            return Err(QueueError::Full);
        }
    }
    Ok(())
}

fn lane(message: &Incoming) -> usize {
    let method = match message {
        Incoming::Text(
            ClientMessage::Request { method, .. }
            | ClientMessage::Event { method, .. }
            | ClientMessage::Response { method, .. },
            _,
        ) => method,
        Incoming::Binary(bytes) => {
            return if bytes.first().is_some_and(|byte| *byte < 0x10) {
                1
            } else {
                2
            };
        }
        Incoming::Text(ClientMessage::Hello(_), _) => return 0,
    };
    if terminal::capabilities::implemented_methods().any(|spec| spec.name == method)
        || voice::capabilities::implemented_methods().any(|spec| spec.name == method)
    {
        return 1;
    }
    if filesystem::capabilities::implemented_methods().any(|spec| spec.name == method) {
        return 2;
    }
    if provider::capabilities::implemented_methods().any(|spec| spec.name == method) {
        return if file_operation(method) { 2 } else { 3 };
    }
    if metadata::capabilities::implemented_methods().any(|spec| spec.name == method)
        && file_operation(method)
    {
        return 2;
    }
    0
}

/// Owner-declared methods that run long checkout, clone or skill installation work keep the
/// file-operation lane, so they cannot block ordinary directory reads or native Agent requests.
fn file_operation(method: &str) -> bool {
    const PREFIXES: [&str; 5] = [
        "agent.skills.",
        "workspace.worktree.",
        "workspace.recovery.",
        "workspace.github.",
        "project.github.",
    ];
    PREFIXES.iter().any(|prefix| method.starts_with(prefix))
}

#[cfg(test)]
mod tests;
