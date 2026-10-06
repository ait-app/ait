use std::collections::BTreeSet;

use serde_json::Value;
use tokio::sync::oneshot;

use super::{Command, ErrorCode, ExecutionState, Router};
use crate::service::agent_execution::lane::{Fence, Message};
use crate::service::agent_manager::auto_archive::Retirement;
use crate::service::agent_manager::ownership::Barrier;

struct PendingFence {
    ready: oneshot::Receiver<()>,
    release: oneshot::Sender<()>,
    done: oneshot::Receiver<Result<(), ErrorCode>>,
    finish: oneshot::Sender<()>,
}

pub(super) fn is_mutation(method: &str) -> bool {
    matches!(
        method,
        "agent.archive.request"
            | "agent.delete.request"
            | "agent.items.close.request"
            | "internal.workspace.retire"
    )
}

pub(super) fn scopes(
    state: &ExecutionState,
    method: &str,
    params: &Value,
) -> Result<BTreeSet<String>, ErrorCode> {
    let records = state.registry.list().map_err(|_| ErrorCode::AgentIo)?;
    let mut scopes = BTreeSet::new();
    let mut pending = Vec::new();
    if method == "internal.workspace.retire" {
        let workspaces: Vec<String> =
            serde_json::from_value(params.clone()).map_err(|_| ErrorCode::InvalidMessage)?;
        for workspace in &workspaces {
            scopes.insert(format!("workspace:{workspace}"));
        }
        pending.extend(
            records
                .iter()
                .filter(|record| {
                    record
                        .workspace_id
                        .as_ref()
                        .is_some_and(|workspace| workspaces.contains(workspace))
                })
                .map(|record| record.id.clone()),
        );
    } else if method == "agent.items.close.request" {
        let ids: Vec<String> = params
            .get("agentIds")
            .map(|ids| serde_json::from_value(ids.clone()))
            .transpose()
            .map_err(|_| ErrorCode::InvalidMessage)?
            .unwrap_or_default();
        pending.extend(ids.iter().filter_map(|id| state.resolve(id).ok()));
    } else {
        pending.push(
            state.resolve(
                params["agentId"]
                    .as_str()
                    .ok_or(ErrorCode::InvalidMessage)?,
            )?,
        );
    }
    let cascade = method != "agent.delete.request";
    let mut seen = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        scopes.insert(format!("lane:{}", state.owners.agent(&id)?));
        if cascade {
            scopes.insert(format!("parent:{id}"));
            pending.extend(
                records
                    .iter()
                    .filter(|record| record.labels.get("paseo.parent-agent-id") == Some(&id))
                    .map(|record| record.id.clone()),
            );
        }
    }
    Ok(scopes)
}

impl Router {
    pub(in crate::service::agent_execution) async fn retirements(&mut self) {
        let template = self.template.clone();
        let prepared = tokio::task::spawn_blocking(move || {
            let state = template.lock().map_err(|_| ErrorCode::AgentIo)?.fork(None);
            let actions = state
                .owners
                .retirements()?
                .into_iter()
                .map(|action| {
                    let params = serde_json::json!({"agentId":action.id});
                    let scoped = scopes(&state, "agent.archive.request", &params)
                        .or_else(|error| {
                            if error == ErrorCode::AgentNotFound {
                                Ok(BTreeSet::new())
                            } else {
                                Err(error)
                            }
                        })
                        .and_then(|mut scoped| {
                            if let Some(workspace) = action.workspace() {
                                scoped.extend(scopes(
                                    &state,
                                    "internal.workspace.retire",
                                    &serde_json::json!([workspace]),
                                )?);
                            }
                            Ok(scoped)
                        });
                    (action, params, scoped)
                })
                .collect::<Vec<_>>();
            Ok::<_, ErrorCode>((state, actions))
        })
        .await;
        let Ok(Ok((state, actions))) = prepared else {
            return;
        };
        for (action, params, scoped) in actions {
            let permit = self.retirements.clone().try_acquire_owned();
            let (Ok(scoped), Ok(permit)) = (scoped, permit) else {
                state.owners.finish_retirement(&action.id, false);
                continue;
            };
            let (reply, _) = oneshot::channel();
            self.mutate_inner(
                state.fork(None),
                Command::Request {
                    method: "agent.archive.request".to_owned(),
                    params,
                    reply,
                    cancel: None,
                    permit,
                    queued: std::time::Instant::now(),
                },
                scoped,
                Some(action),
            );
        }
    }

    pub(super) fn mutate(
        &mut self,
        state: ExecutionState,
        command: Command,
        scopes: BTreeSet<String>,
    ) {
        self.mutate_inner(state, command, scopes, None);
    }

    fn mutate_inner(
        &mut self,
        state: ExecutionState,
        command: Command,
        scopes: BTreeSet<String>,
        retirement: Option<Retirement>,
    ) {
        let prepared = self.fences(&state, scopes);
        let (barrier, pending) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                if let Some(action) = retirement {
                    state.owners.finish_retirement(&action.id, false);
                }
                super::reject(command, error);
                return;
            }
        };
        self.tasks.spawn(async move {
            let _barrier = barrier;
            let Command::Request {
                method,
                params,
                reply,
                permit,
                queued,
                ..
            } = command
            else {
                return;
            };
            let _permit = permit;
            let mut releases = Vec::with_capacity(pending.len());
            let mut completions = Vec::with_capacity(pending.len());
            let mut finishes = Vec::with_capacity(pending.len());
            let mut ready_error = None;
            for fence in pending {
                if fence.ready.await.is_err() {
                    ready_error.get_or_insert(ErrorCode::AgentIo);
                }
                releases.push(fence.release);
                completions.push(fence.done);
                finishes.push(fence.finish);
            }
            let started = std::time::Instant::now();
            let queue_ms = queued.elapsed().as_millis();
            let measured_method = method.clone();
            let handle = tokio::runtime::Handle::current();
            let owners = state.owners.clone();
            let workspace_retirement = retirement.is_some();
            let workspace = retirement
                .as_ref()
                .and_then(Retirement::workspace)
                .map(str::to_owned);
            let result = tokio::task::spawn_blocking(move || {
                handle.block_on(async move {
                    if let Some(error) = ready_error {
                        return Err(error);
                    }
                    let mut state = state;
                    if let Some(workspace) = workspace {
                        state
                            .manager
                            .archive_workspace_agents(&[workspace])
                            .map_err(|_| ErrorCode::AgentIo)?;
                    }
                    let result = state.execute(&method, params).await;
                    if workspace_retirement && matches!(result, Err(ErrorCode::AgentNotFound)) {
                        return Ok(serde_json::json!({}));
                    }
                    result
                })
            })
            .await
            .unwrap_or(Err(ErrorCode::AgentIo));
            for release in releases {
                let _ = release.send(());
            }
            let mut close_error = None;
            for done in completions {
                if let Err(error) = done.await.unwrap_or(Err(ErrorCode::AgentIo)) {
                    close_error.get_or_insert(error);
                }
            }
            let mut result = result.and_then(|value| close_error.map_or(Ok(value), Err));
            result = cleanup(&owners, retirement, result).await;
            tracing::debug!(
                method = measured_method,
                class = "lifecycle",
                queue_ms,
                execution_ms = started.elapsed().as_millis(),
                "provider.request.completed"
            );
            for finish in finishes {
                let _ = finish.send(());
            }
            let _ = reply.send(result);
        });
    }

    fn fences(
        &self,
        state: &ExecutionState,
        scopes: BTreeSet<String>,
    ) -> Result<(Barrier, Vec<PendingFence>), ErrorCode> {
        let mut lanes = state.owners.related(&scopes)?;
        lanes.extend(
            scopes
                .iter()
                .filter_map(|scope| scope.strip_prefix("lane:").map(str::to_owned)),
        );
        let barrier = state.owners.freeze(scopes.into_iter().collect())?;
        let mut pending = Vec::new();
        for lane in lanes {
            let Some(handle) = self
                .lanes
                .get(&lane)
                .filter(|handle| !handle.sender.is_closed())
            else {
                continue;
            };
            // A newly admitted lane has no native resources until its earlier barrier opens.
            if handle.barriers.iter().any(|token| !token.is_cancelled()) {
                continue;
            }
            let (ready, waiting) = oneshot::channel();
            let (release, resume) = oneshot::channel();
            let (done, completion) = oneshot::channel();
            let (finish, complete) = oneshot::channel();
            handle
                .sender
                .try_send(Message::Fence(Fence {
                    ready,
                    resume,
                    done,
                    complete,
                }))
                .map_err(|_| ErrorCode::CatalogBusy)?;
            pending.push(PendingFence {
                ready: waiting,
                release,
                done: completion,
                finish,
            });
        }
        Ok((barrier, pending))
    }
}

async fn cleanup(
    owners: &crate::service::agent_manager::ownership::Owners,
    action: Option<Retirement>,
    mut result: Result<Value, ErrorCode>,
) -> Result<Value, ErrorCode> {
    if let Some(action) = action {
        if result.is_ok() {
            let cleanup = action.clone();
            if let Err(error) = tokio::task::spawn_blocking(move || cleanup.cleanup())
                .await
                .unwrap_or(Err(ErrorCode::AgentIo))
            {
                result = Err(error);
            }
        }
        owners.finish_retirement(&action.id, result.is_ok());
    }
    result
}
