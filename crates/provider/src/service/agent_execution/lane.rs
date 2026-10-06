use std::time::{Duration, Instant};

use tokio::sync::{mpsc, oneshot};

use super::{Command, ErrorCode, ExecutionState, voice};

pub(super) struct Fence {
    pub(super) ready: oneshot::Sender<()>,
    pub(super) resume: oneshot::Receiver<()>,
    pub(super) done: oneshot::Sender<Result<(), ErrorCode>>,
    pub(super) complete: oneshot::Receiver<()>,
}

pub(super) enum Message {
    Fence(Fence),
    Request(Command),
    Shutdown(oneshot::Sender<Result<(), ErrorCode>>),
}

pub(super) async fn serve(
    mut state: ExecutionState,
    mut commands: mpsc::Receiver<Message>,
    barriers: Vec<tokio_util::sync::CancellationToken>,
) {
    for barrier in barriers {
        barrier.cancelled().await;
    }
    let mut interval = tokio::time::interval(Duration::from_millis(25));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut activity = Instant::now();
    let mut idle_poll = Instant::now();
    loop {
        let step = tokio::select! {
            message = commands.recv() => match message {
                Some(Message::Fence(fence)) => {
                    let _ = state.owners.pending(&state.manager.owned_agents());
                    let _ = fence.ready.send(());
                    let _ = fence.resume.await;
                    match reconcile(state).await {
                        Ok((next, result)) => {
                            let _ = fence.done.send(result);
                            let _ = fence.complete.await;
                            Ok(next)
                        }
                        Err(error) => { let _ = fence.done.send(Err(error)); Err(error) }
                    }
                }
                Some(Message::Request(command)) => {
                    activity = Instant::now();
                    request(state, command, true).await
                }
                Some(Message::Shutdown(reply)) => {
                    let result = close(state).await;
                    let _ = reply.send(result);
                    break;
                }
                None => { let _ = close(state).await; break; }
            },
            _ = interval.tick() => {
                if commands.is_empty() && !state.manager.has_sessions() && activity.elapsed() > Duration::from_secs(30) {
                    commands.close();
                    let _ = close(state).await;
                    break;
                }
                if !state.manager.has_sessions() && Instant::now() < idle_poll {
                    Ok(state)
                } else {
                    idle_poll = Instant::now() + Duration::from_millis(250);
                    poll(state).await
                }
            }
        };
        match step {
            Ok(next) => state = next,
            Err(_) => break,
        }
    }
}

async fn reconcile(
    mut state: ExecutionState,
) -> Result<(ExecutionState, Result<(), ErrorCode>), ErrorCode> {
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        handle.block_on(async move {
            state.observe_messages();
            let result = state
                .manager
                .reconcile()
                .await
                .map_err(|_| ErrorCode::AgentIo);
            let _ = state.publish();
            (state, result)
        })
    })
    .await
    .map_err(|_| ErrorCode::AgentIo)
}

pub(super) async fn request(
    mut state: ExecutionState,
    command: Command,
    owned: bool,
) -> Result<ExecutionState, ErrorCode> {
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        handle.block_on(async move {
            if let Command::Request {
                method,
                params,
                reply,
                cancel,
                permit,
                queued,
            } = command
            {
                let _permit = permit;
                let started = Instant::now();
                let queue_ms = queued.elapsed().as_millis();
                if owned {
                    let _ = state.owners.pending(&state.manager.owned_agents());
                    state.observe_messages();
                    let prepared = async {
                        state.manager.reconcile().await?;
                        state.manager.poll().await
                    }
                    .await;
                    if prepared.is_err() {
                        let _ = state.publish();
                        let _ = reply.send(Err(ErrorCode::AgentIo));
                        return state;
                    }
                }
                voice::dispatch(&mut state, &method, params, (reply, cancel)).await;
                tracing::debug!(
                    method,
                    class = class(&method, owned),
                    queue_ms,
                    execution_ms = started.elapsed().as_millis(),
                    "provider.request.completed"
                );
            }
            state
        })
    })
    .await
    .map_err(|_| ErrorCode::AgentIo)
}

async fn poll(mut state: ExecutionState) -> Result<ExecutionState, ErrorCode> {
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        handle.block_on(async move {
            let started = Instant::now();
            state.observe_messages();
            let _ = state.manager.reconcile().await;
            let _ = state.manager.poll().await;
            let _ = state.manager.dispatch_pending_inputs().await;
            let _ = state.publish();
            if state.manager.has_sessions() {
                tracing::debug!(
                    elapsed_ms = started.elapsed().as_millis(),
                    "provider.events.committed"
                );
            }
            state
        })
    })
    .await
    .map_err(|_| ErrorCode::AgentIo)
}

fn class(method: &str, owned: bool) -> &'static str {
    if method.starts_with("provider.") {
        return "catalog";
    }
    if method.starts_with("agent.timeline.") && owned {
        return "history";
    }
    if !owned {
        return "read";
    }
    if matches!(
        method,
        "agent.create.request"
            | "agent.resume.request"
            | "agent.refresh.request"
            | "agent.rewind.request"
            | "internal.workspace.agent.create"
    ) {
        return "lifecycle";
    }
    "foreground"
}

async fn close(mut state: ExecutionState) -> Result<(), ErrorCode> {
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        handle.block_on(async move {
            state.observe_messages();
            let result = state
                .manager
                .close_all()
                .await
                .map_err(|_| ErrorCode::AgentIo);
            let _ = state.publish();
            state.manager.release_owner();
            result
        })
    })
    .await
    .map_err(|_| ErrorCode::AgentIo)?
}
