use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};

use super::{HostEvent, Launch, Record};
use crate::ports::agent_session::{AgentSession, AgentSessionError, AgentTurnEvent};
use crate::ports::controls::SubagentEvent;
use crate::protocol::prompt::AgentPrompt;

/// Commands accepted by the single actor owning an external worker session.
#[derive(Debug)]
pub(super) enum Command {
    Send(String, oneshot::Sender<Result<(), AgentSessionError>>),
    Kill(oneshot::Sender<()>),
    Permission(
        String,
        Value,
        oneshot::Sender<Result<(), AgentSessionError>>,
    ),
}

/// Own one external worker writer until explicitly closed or its parent disappears.
pub(super) async fn run(
    launch: Launch,
    mut commands: mpsc::Receiver<Command>,
    events: mpsc::Sender<(u64, HostEvent)>,
    ready: oneshot::Sender<Result<Record, AgentSessionError>>,
) {
    let generation = launch.generation;
    let (mut session, mut record, initial_turn) = match open(&launch).await {
        Ok(opened) => opened,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    record.status("running");
    record.child.persistence = session.persistence();
    let id = record.child.id.clone();
    if ready.send(Ok(record.clone())).is_err() {
        let _ = session.close().await;
        return;
    }
    if events
        .send((generation, HostEvent::Record(Box::new(record.clone()))))
        .await
        .is_err()
    {
        let _ = session.close().await;
        return;
    }
    let mut turn = Some(initial_turn);
    let mut polling = tokio::time::interval(Duration::from_millis(20));
    let mut running = true;
    loop {
        tokio::select! {
            command = commands.recv() => match command {
                Some(Command::Send(text, reply)) => {
                    if running {
                        let result = match &turn {
                            Some(turn) => session.steer_turn(turn, &text).await,
                            None => Err(AgentSessionError::Rejected),
                        };
                        let _ = reply.send(result); continue;
                    }
                    let sent = session.start_input(&AgentPrompt::text(&text), &launch.spec.config).await;
                    if let Ok(next) = &sent {
                        turn = Some(next.clone()); running = true; record.answer.clear(); record.status("running"); record.child.persistence = session.persistence();
                        let _ = events.send((generation, HostEvent::Record(Box::new(record.clone())))).await;
                    }
                    let _ = reply.send(sent.map(|_| ()));
                }
                Some(Command::Permission(request, response, reply)) => {
                    let _ = reply.send(session.respond_permission(&request, &response).await);
                }
                Some(Command::Kill(reply)) => {
                    if let Some(turn) = &turn { let _ = session.cancel_turn(turn).await; }
                    let _ = session.close().await;
                    record.status("canceled");
                    let _ = events.send((generation, HostEvent::Record(Box::new(record.clone())))).await;
                    let _ = reply.send(()); return;
                }
                None => { let _ = session.close().await; return; }
            },
            _ = polling.tick(), if running => {
                for _ in 0..64 {
                    let event = match session.poll_turn() {
                        Ok(Some(event)) => event,
                        Ok(None) => break,
                        Err(_) => {
                            record.status("failed"); running = false;
                            let _ = events.send((generation, HostEvent::Record(Box::new(record.clone())))).await;
                            break;
                        }
                    };
                    record.child.persistence = session.persistence();
                    let projected = match event {
                        AgentTurnEvent::Completed(answer) => {
                            record.answer = clip(answer.as_deref().unwrap_or_default(), 16 * 1024);
                            record.status("completed"); running = false;
                            let _ = events.send((generation, HostEvent::Record(Box::new(record.clone())))).await; None
                        }
                        AgentTurnEvent::Failed => {
                            record.status("failed");running=false;let _=events.send((generation, HostEvent::Record(Box::new(record.clone())))).await;None
                        }
                        AgentTurnEvent::Cancelled => {
                            record.status("canceled"); running = false;
                            let _ = events.send((generation, HostEvent::Record(Box::new(record.clone())))).await; None
                        }
                        AgentTurnEvent::Timeline(entry) => Some(AgentTurnEvent::Subagent(SubagentEvent::Timeline {id:id.clone(),entry})),
                        AgentTurnEvent::Progress { observation, entry } => Some(AgentTurnEvent::Subagent(SubagentEvent::Progress {id:id.clone(),observation,entry})),
                        AgentTurnEvent::PermissionRequested(request) => {
                            let native = request.get("id").and_then(Value::as_str).unwrap_or_default();
                            let host = format!("ait-vibe:{id}:{native}");
                            let mut question = request.clone(); question["id"] = json!(host);
                            let _ = events.send((generation, HostEvent::Permission {host, worker:id.clone(), native:native.to_owned(), request:question})).await; None
                        }
                        _ => None,
                    };
                    if let Some(event) = projected && events.send((generation, HostEvent::Event(event))).await.is_err() { let _ = session.close().await; return; }
                }
            }
        }
    }
}

fn clip(text: &str, max: usize) -> String {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

async fn open(
    launch: &Launch,
) -> Result<(crate::local::omp::session::Session, Record, String), AgentSessionError> {
    let mut session = Box::pin(crate::local::omp::session::open_worker(
        &launch.client,
        &launch.spec,
        &launch.parent_file,
        launch.restore.as_ref(),
    ))
    .await?;
    let handle = session.persistence().ok_or(AgentSessionError::Failed)?;
    let record = launch.record(&handle);
    let turn = session
        .start_input(&AgentPrompt::text(&launch.prompt), &launch.spec.config)
        .await?;
    Ok((session, record, turn))
}
