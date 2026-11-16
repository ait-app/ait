//! Host coordination between schedules, independent Agent execution and workspace ownership.
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::Context;
use bot::mcp::Scopes;
use domain::bot::permissions::Holder;
use domain::bot::{BOT_CONVERSATION_LABEL, BOT_SCOPE_LABEL};
use domain::input_envelope;
use domain::schedule::{Run, RunStatus, Schedule, Target};
use domain::workspace::lifecycle::WorkspaceCreation;
use domain::workspace::records::PersistedWorkspaceRecord;
use metadata::directory::service::directory::Directory;
use metadata::worktrees::service::worktrees::{CreateAction, CreateWorktree, Worktrees};
use model::storage::schedule::Store;
use persistence::storage::schedule::FileStore;
use provider::service::agent_execution::AgentExecution;
use schedule::ports::{Outcome, Progress, Runner};
use schedule::service::Schedules;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::notifications::{Notifier, schedule_notification};

/// Upper bound for one completion wait; pending permissions are re-checked after each wait.
const FINISH_WAIT_MS: u64 = 1_000;
/// Failure of an occurrence whose Bot no longer grants its schedule; the task stays active.
const REVOKED: &str = "Workspace Bot permissions no longer allow this schedule";

/// Agent RPCs a scheduled occurrence performs, served by the host's Agent execution.
#[async_trait::async_trait]
trait AgentRpc: Send + Sync + std::fmt::Debug {
    /// Execute Agent RPC `method` with `params`.
    async fn execute(&self, method: &str, params: Value)
    -> Result<Value, provider::rpc::ErrorCode>;
}

#[async_trait::async_trait]
impl AgentRpc for AgentExecution {
    async fn execute(
        &self,
        method: &str,
        params: Value,
    ) -> Result<Value, provider::rpc::ErrorCode> {
        Self::execute(self, method, params).await
    }
}

#[derive(Debug)]
struct AgentRunner {
    execution: Arc<dyn AgentRpc>,
    directory: Directory,
    worktrees: Arc<Mutex<Worktrees>>,
    /// Current Bot grants, re-read before every occurrence of a Bot-scoped task.
    scopes: Arc<Scopes>,
    /// Receives one notification per settled occurrence; absent without a runtime.
    notifier: Option<Notifier>,
}
struct Occurrence<'a> {
    run_id: &'a str,
    progress: &'a Progress,
    cancel: &'a CancellationToken,
}

/// Start the schedule worker, running occurrences through `execution` in `directory`'s
/// Workspaces; Bot-scoped tasks are re-checked against `scopes` before every occurrence, and
/// every settled occurrence is offered to `notifier`.
/// # Errors
/// Returns data-directory, interrupted-run recovery or worker start failures.
pub(super) fn compose(
    data: &Path,
    execution: AgentExecution,
    directory: Directory,
    worktrees: Arc<Mutex<Worktrees>>,
    (scopes, notifier): (Arc<Scopes>, Option<Notifier>),
) -> anyhow::Result<Schedules> {
    let data = data
        .canonicalize()
        .context("resolve schedule data directory")?;
    let store = FileStore::new(data.join("schedules/schedules.json"));
    // Recover owned workspaces before marking interrupted occurrences settled.
    recover_interrupted(&store.load()?, &execution, &directory)?;
    Schedules::spawn(
        Box::new(store),
        Arc::new(AgentRunner {
            execution: Arc::new(execution),
            directory,
            worktrees,
            scopes,
            notifier,
        }),
    )
    .context("start schedule worker")
}
/// Clean up resources owned by occurrences that were running when the daemon stopped.
///
/// Cleanup is best effort: an Agent or Workspace that is already gone counts as recovered, and
/// other failures are logged so a stale run record can never prevent the daemon from starting.
/// # Errors
/// Returns an error only when the recovery runtime cannot be created.
fn recover_interrupted(
    schedules: &[Schedule],
    execution: &AgentExecution,
    directory: &Directory,
) -> anyhow::Result<()> {
    let interrupted = schedules
        .iter()
        .filter_map(|schedule| match &schedule.target {
            Target::NewAgent { config } => Some((schedule, config)),
            Target::Agent { .. } => None,
        })
        .flat_map(|(schedule, config)| {
            schedule
                .runs
                .iter()
                .filter(move |run| {
                    run.status == RunStatus::Running
                        && (run.agent_id.is_none() || config["archiveOnFinish"] != false)
                })
                .map(move |run| {
                    let workspace = run
                        .workspace_id
                        .as_deref()
                        .filter(|_| schedule.workspace_id.is_none());
                    (run.agent_id.as_deref(), workspace)
                })
        })
        .collect::<Vec<_>>();
    if interrupted.is_empty() {
        return Ok(());
    }
    // No live provider process exists during composition; archive through its owner.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("start scheduled run recovery")?;
    let archived_at = chrono::Utc::now().to_rfc3339();
    for (agent, workspace) in interrupted {
        if let Some(id) = agent {
            runtime.block_on(archive_agent(execution, id));
        }
        if let Some(id) = workspace
            && let Err(error) = directory.archive_workspace(id, &archived_at)
        {
            tracing::warn!(?error, "scheduled workspace recovery failed");
        }
    }
    Ok(())
}

/// Archive a scheduled Agent; an Agent that no longer exists needs no cleanup.
async fn archive_agent(execution: &dyn AgentRpc, id: &str) {
    match execution
        .execute("agent.archive.request", json!({"agentId":id}))
        .await
    {
        Ok(_) | Err(provider::rpc::ErrorCode::AgentNotFound) => {}
        Err(error) => {
            tracing::warn!(error_code = ?error, agent_id = id, "scheduled agent archive failed");
        }
    }
}
#[async_trait::async_trait]
impl Runner for AgentRunner {
    async fn resolve(
        &self,
        workspace_id: String,
        mut target: Target,
    ) -> Result<Target, domain::storage::schedule::Error> {
        use domain::storage::schedule::Error;
        let workspace = self
            .workspace(&workspace_id)
            .await
            .map_err(|_| Error::Invalid)?;
        match &mut target {
            Target::Agent { agent_id } => {
                let value = self
                    .execution
                    .execute("agent.get.request", json!({"agentId":agent_id}))
                    .await
                    .map_err(|_| Error::Invalid)?;
                if !available_agent(&value)
                    || value["agent"]["workspaceId"].as_str() != Some(&workspace_id)
                {
                    return Err(Error::Invalid);
                }
            }
            Target::NewAgent { config } => {
                if config["isolation"] == "worktree" {
                    return Err(Error::Invalid);
                }
                let object = config.as_object_mut().ok_or(Error::Invalid)?;
                object.insert("cwd".into(), json!(workspace.cwd));
                object.remove("isolation");
            }
        }
        Ok(target)
    }
    async fn run(
        &self,
        schedule: Schedule,
        run_id: String,
        progress: Progress,
        cancel: CancellationToken,
    ) -> Outcome {
        let mut outcome = Outcome::default();
        let occurrence = Occurrence {
            run_id: &run_id,
            progress: &progress,
            cancel: &cancel,
        };
        let result = self.perform(&schedule, &occurrence, &mut outcome).await;
        if let Err(error) = result {
            outcome.error = Some(error.to_owned());
        }
        if let Target::NewAgent { config } = &schedule.target
            && (outcome.agent_id.is_none() || config["archiveOnFinish"] != false)
        {
            if let Some(id) = &outcome.agent_id {
                archive_agent(self.execution.as_ref(), id).await;
            }
            if schedule.workspace_id.is_none()
                && let Some(id) = outcome.workspace_id.clone()
            {
                let directory = self.directory.clone();
                let archived = tokio::task::spawn_blocking(move || {
                    directory.archive_workspace(&id, &chrono::Utc::now().to_rfc3339())
                })
                .await;
                if !matches!(archived, Ok(Ok(_))) {
                    outcome
                        .error
                        .get_or_insert_with(|| "Scheduled workspace cleanup failed".into());
                }
            }
        }
        outcome
    }
    fn settled(&self, schedule: &Schedule, run: &Run) {
        if let Some(notifier) = &self.notifier
            && let Some(notification) = schedule_notification(schedule, run)
        {
            notifier.offer(notification);
        }
    }
}
impl AgentRunner {
    async fn workspace(&self, id: &str) -> Result<PersistedWorkspaceRecord, &'static str> {
        let directory = self.directory.clone();
        let id = id.to_owned();
        tokio::task::spawn_blocking(move || {
            let workspace = directory
                .list_workspaces()
                .map_err(|_| "Cannot load scheduled workspace")?
                .into_iter()
                .find(|workspace| workspace.workspace_id == id && !workspace.is_archived())
                .ok_or("Scheduled workspace is missing or archived")?;
            if !Path::new(&workspace.cwd).is_dir() {
                return Err("Scheduled workspace directory no longer exists");
            }
            Ok(workspace)
        })
        .await
        .map_err(|_| "Cannot load scheduled workspace")?
    }
    /// Re-check Bot-scoped `schedule` against its root's current grant before it acts.
    ///
    /// Returns the mode a new Agent must run in: the stored mode, or the cap's default when the
    /// task stores none. Tasks outside a Bot scope and heartbeats return `None`.
    /// # Errors
    /// Returns `REVOKED` when the grant no longer includes schedules or the task's mode.
    fn authorize(&self, schedule: &Schedule) -> Result<Option<String>, &'static str> {
        let Some(root) = &schedule.bot_scope else {
            return Ok(None);
        };
        let permissions = self.scopes.grant(root).permissions;
        if !permissions.allows("create_schedule", Holder::Subtree) {
            return Err(REVOKED);
        }
        match &schedule.target {
            Target::Agent { .. } => Ok(None),
            Target::NewAgent { config } => {
                let provider = config["provider"].as_str().ok_or(REVOKED)?;
                permissions
                    .mode(provider, config["modeId"].as_str())
                    .map(Some)
                    .map_err(|_| REVOKED)
            }
        }
    }
    async fn create_agent(
        &self,
        schedule: &Schedule,
        mut config: Value,
        occurrence: &Occurrence<'_>,
        outcome: &mut Outcome,
    ) -> Result<(), &'static str> {
        let workspace = if let Some(id) = &schedule.workspace_id {
            match self.workspace(id).await {
                Ok(workspace) => workspace,
                Err(error) => {
                    outcome.target_gone = true;
                    return Err(error);
                }
            }
        } else {
            // Version-one tasks retain their original dedicated-Workspace execution until explicitly bound.
            let cwd = config["cwd"]
                .as_str()
                .ok_or("Invalid scheduled directory")?
                .to_owned();
            if !Path::new(&cwd).is_dir() {
                outcome.target_gone = true;
                return Err("Scheduled directory no longer exists");
            }
            let worktree = config["isolation"] == "worktree";
            let directory = self.directory.clone();
            let worktrees = self.worktrees.clone();
            let prompt = schedule.prompt.clone();
            tokio::task::spawn_blocking(move || {
                let now = chrono::Utc::now().to_rfc3339();
                if worktree {
                    worktrees
                        .lock()
                        .map_err(|_| "Workspace lock failed")?
                        .create(
                            &CreateWorktree {
                                workspace_id: None,
                                title: None,
                                branch_name: None,
                                base_branch: None,
                                cwd,
                                project_id: None,
                                worktree_slug: None,
                                ref_name: None,
                                action: CreateAction::BranchOff,
                                checkout_source: None,
                                first_agent_prompt: Some(prompt),
                                expects_initial_agent: true,
                                sandbox: None,
                            },
                            &now,
                        )
                        .map(|result| result.workspace)
                        .map_err(|_| "Scheduled worktree creation failed")
                } else {
                    directory
                        .create_workspace(WorkspaceCreation {
                            path: &cwd,
                            title: Some(prompt),
                            project_id: None,
                            workspace_id: None,
                            expects_initial_agent: true,
                            timestamp: &now,
                        })
                        .map_err(|_| "Scheduled workspace creation failed")
                }
            })
            .await
            .map_err(|_| "Scheduled workspace creation failed")??
        };
        outcome.workspace_id = Some(workspace.workspace_id.clone());
        occurrence
            .progress
            .record(None, outcome.workspace_id.clone())
            .await
            .map_err(|_| "Cannot persist scheduled workspace")?;
        if occurrence.cancel.is_cancelled() {
            return Err("Scheduled run canceled");
        }
        let object = config
            .as_object_mut()
            .ok_or("Invalid agent configuration")?;
        object.remove("archiveOnFinish");
        object.remove("isolation");
        config["cwd"] = json!(workspace.cwd);
        let mut labels =
            json!({"paseo.schedule-id":schedule.id,"paseo.schedule-run":occurrence.run_id});
        // Bot-scoped Agents receive derived credentials instead of the host token.
        if let Some(root) = &schedule.bot_scope {
            labels[BOT_SCOPE_LABEL] = json!(root);
        }
        // Notifications about the Agent's own permission requests reach the creating conversation.
        if let Some(conversation) = schedule.bot_conversation {
            labels[BOT_CONVERSATION_LABEL] = json!(conversation);
        }
        let created=self.execution.execute("agent.create.request",json!({"config":config,"workspaceId":workspace.workspace_id,"idempotencyKey":occurrence.run_id,"labels":labels})).await.map_err(|_|"Scheduled agent creation failed")?;
        outcome.agent_id = created["agentId"].as_str().map(str::to_owned);
        if outcome.agent_id.is_none() {
            return Err("Scheduled agent creation failed");
        }
        occurrence
            .progress
            .record(outcome.agent_id.clone(), outcome.workspace_id.clone())
            .await
            .map_err(|_| "Cannot persist scheduled agent")?;
        Ok(())
    }
    async fn perform(
        &self,
        schedule: &Schedule,
        occurrence: &Occurrence<'_>,
        outcome: &mut Outcome,
    ) -> Result<(), &'static str> {
        let Occurrence {
            run_id,
            progress,
            cancel,
        } = occurrence;
        if cancel.is_cancelled() {
            return Err("Scheduled run canceled");
        }
        let mode = self.authorize(schedule)?;
        let text = match &schedule.target {
            Target::Agent { agent_id } => {
                if let Some(id) = &schedule.workspace_id {
                    match self.workspace(id).await {
                        Ok(_) => outcome.workspace_id = Some(id.clone()),
                        Err(error) => {
                            outcome.target_gone = true;
                            return Err(error);
                        }
                    }
                }
                let value = self
                    .execution
                    .execute("agent.get.request", json!({"agentId":agent_id}))
                    .await;
                let value = match value {
                    Ok(value) => value,
                    Err(provider::rpc::ErrorCode::AgentNotFound) => {
                        outcome.target_gone = true;
                        return Err("Scheduled agent no longer exists");
                    }
                    Err(_) => return Err("Cannot load scheduled agent"),
                };
                if !available_agent(&value) {
                    outcome.target_gone = true;
                    return Err("Scheduled agent is missing, archived, or read-only");
                }
                if schedule
                    .workspace_id
                    .as_deref()
                    .is_some_and(|id| value["agent"]["workspaceId"].as_str() != Some(id))
                {
                    outcome.target_gone = true;
                    return Err("Scheduled agent no longer belongs to this workspace");
                }
                outcome.agent_id = Some(agent_id.clone());
                progress
                    .record(outcome.agent_id.clone(), outcome.workspace_id.clone())
                    .await
                    .map_err(|_| "Cannot persist scheduled agent")?;
                input_envelope::schedule(
                    &schedule.id,
                    run_id,
                    schedule.name.as_deref(),
                    &schedule.prompt,
                )
            }
            Target::NewAgent { config } => {
                let mut config = config.clone();
                if let Some(mode) = mode {
                    config["modeId"] = json!(mode);
                }
                self.create_agent(schedule, config, occurrence, outcome)
                    .await?;
                // A new Agent's prompt stays bare so a slash command in it still runs.
                schedule.prompt.clone()
            }
        };
        let id = outcome
            .agent_id
            .as_deref()
            .ok_or("Scheduled agent missing")?
            .to_owned();
        self.send_and_wait(&id, text, cancel, outcome).await
    }

    async fn send_and_wait(
        &self,
        id: &str,
        text: String,
        cancel: &CancellationToken,
        outcome: &mut Outcome,
    ) -> Result<(), &'static str> {
        if cancel.is_cancelled() {
            return Err("Scheduled run canceled");
        }
        let sent = self
            .execution
            .execute(
                "agent.message.send.request",
                json!({"agentId":id,"text":text}),
            )
            .await
            .map_err(|_| "Scheduled agent send failed")?;
        if sent["accepted"] != true {
            return Err("Scheduled agent is busy or rejected the prompt");
        }
        loop {
            let waited = tokio::select! {
                ()=cancel.cancelled()=>{let _=self.execution.execute("agent.cancel.request",json!({"agentId":id})).await;return Err("Scheduled run canceled");},
                result=self.execution.execute("agent.finish.wait.request",json!({"agentId":id,"timeoutMs":FINISH_WAIT_MS}))=>result.map_err(|_|"Scheduled agent wait failed")?,
            };
            if waited["final"]["pendingPermissions"]
                .as_array()
                .is_some_and(|a| !a.is_empty())
                || waited["final"]["pendingPermissionRequests"]
                    .as_array()
                    .is_some_and(|a| !a.is_empty())
            {
                let _ = self
                    .execution
                    .execute("agent.cancel.request", json!({"agentId":id}))
                    .await;
                return Err("Scheduled agent is waiting for permission");
            }
            match waited["status"].as_str() {
                // A timed-out wait already blocked for the full interval; only re-check permissions.
                Some("timeout") => {}
                Some("running") => tokio::time::sleep(Duration::from_millis(25)).await,
                Some("idle") => {
                    outcome.output = waited["lastMessage"].as_str().map(str::to_owned);
                    return Ok(());
                }
                _ => return Err("Scheduled agent execution failed"),
            }
        }
    }
}

fn available_agent(value: &Value) -> bool {
    value["agent"].is_object()
        && value["agent"]["archivedAt"].is_null()
        && value["agent"]["management"] != "observed"
        && value["agent"]["capabilities"]["supportsMessageSend"] != false
}

#[cfg(test)]
mod tests;
