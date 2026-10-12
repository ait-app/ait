//! Host coordination between schedules, independent Agent execution and workspace ownership.
use std::{
    future::Future,
    path::Path,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::Context;
use domain::schedule::{RunStatus, Schedule, Target};
use domain::workspace::lifecycle::WorkspaceCreation;
use domain::workspace::records::PersistedWorkspaceRecord;
use metadata::directory::service::directory::Directory;
use metadata::worktrees::service::worktrees::{CreateAction, CreateWorktree, Worktrees};
use model::storage::schedule::Store;
use persistence::storage::schedule::FileStore;
use provider::service::agent_execution::AgentExecution;
use schedule::ports::{Outcome, Progress, Runner};
use schedule::service::Schedules;
use serde_json::json;
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
struct AgentRunner {
    execution: AgentExecution,
    directory: Directory,
    worktrees: Arc<Mutex<Worktrees>>,
}
struct Occurrence<'a> {
    run_id: &'a str,
    progress: &'a Progress,
    cancel: &'a CancellationToken,
}

pub(super) fn compose(
    data: &Path,
    execution: AgentExecution,
    directory: Directory,
    worktrees: Arc<Mutex<Worktrees>>,
) -> anyhow::Result<Schedules> {
    let data = data
        .canonicalize()
        .context("resolve schedule data directory")?;
    let store = FileStore::new(data.join("schedules/schedules.json"));
    // Recover owned workspaces before marking interrupted occurrences settled.
    for schedule in store.load()? {
        if let Target::NewAgent { config } = &schedule.target {
            for run in &schedule.runs {
                if run.status == RunStatus::Running
                    && (run.agent_id.is_none() || config["archiveOnFinish"] != false)
                {
                    if let Some(id) = &run.agent_id {
                        // No live provider process exists during composition; archive through its owner.
                        futures_recover(&execution, id)?;
                    }
                    if schedule.workspace_id.is_none()
                        && let Some(id) = &run.workspace_id
                    {
                        directory.archive_workspace(id, &chrono::Utc::now().to_rfc3339())?;
                    }
                }
            }
        }
    }
    Schedules::spawn(
        Box::new(store),
        Arc::new(AgentRunner {
            execution,
            directory,
            worktrees,
        }),
    )
    .context("start schedule worker")
}
fn futures_recover(execution: &AgentExecution, id: &str) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        execution
            .execute("agent.archive.request", json!({"agentId":id}))
            .await
            .map(|_| ())
            .map_err(|_| anyhow::anyhow!("recover scheduled agent"))
    })
}
impl Runner for AgentRunner {
    fn resolve(
        &self,
        workspace_id: String,
        mut target: Target,
    ) -> Pin<Box<dyn Future<Output = Result<Target, domain::storage::schedule::Error>> + Send + '_>>
    {
        Box::pin(async move {
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
                    if value["agent"].is_null()
                        || !value["agent"]["archivedAt"].is_null()
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
        })
    }
    fn run(
        &self,
        schedule: Schedule,
        run_id: String,
        progress: Progress,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Outcome> + Send + '_>> {
        Box::pin(async move {
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
                    let _ = self
                        .execution
                        .execute("agent.archive.request", json!({"agentId":id}))
                        .await;
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
        })
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
                .find(|workspace| workspace.workspace_id == id && workspace.archived_at.is_none())
                .ok_or("Scheduled workspace is missing or archived")?;
            if !Path::new(&workspace.cwd).is_dir() {
                return Err("Scheduled workspace directory no longer exists");
            }
            Ok(workspace)
        })
        .await
        .map_err(|_| "Cannot load scheduled workspace")?
    }
    async fn create_agent(
        &self,
        schedule: &Schedule,
        config: &serde_json::Value,
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
        let mut config = config.clone();
        let object = config
            .as_object_mut()
            .ok_or("Invalid agent configuration")?;
        object.remove("archiveOnFinish");
        object.remove("isolation");
        config["cwd"] = json!(workspace.cwd);
        let created=self.execution.execute("agent.create.request",json!({"config":config,"workspaceId":workspace.workspace_id,"idempotencyKey":occurrence.run_id,"labels":{"paseo.schedule-id":schedule.id,"paseo.schedule-run":occurrence.run_id}})).await.map_err(|_|"Scheduled agent creation failed")?;
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
                if value["agent"].is_null() || !value["agent"]["archivedAt"].is_null() {
                    outcome.target_gone = true;
                    return Err("Scheduled agent is missing or archived");
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
                let heading = schedule.name.as_ref().map_or_else(
                    || format!("Schedule fired (id={}, run={run_id}).", schedule.id),
                    |name| {
                        format!(
                            "Schedule \"{name}\" fired (id={}, run={run_id}).",
                            schedule.id
                        )
                    },
                );
                format!(
                    "<paseo-system>\n{heading}\n{}\n</paseo-system>",
                    schedule.prompt
                )
            }
            Target::NewAgent { config } => {
                self.create_agent(schedule, config, occurrence, outcome)
                    .await?;
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
                result=self.execution.execute("agent.finish.wait.request",json!({"agentId":id,"timeoutMs":250}))=>result.map_err(|_|"Scheduled agent wait failed")?,
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
                Some("timeout" | "running") => tokio::time::sleep(Duration::from_millis(25)).await,
                Some("idle") => {
                    outcome.output = waited["lastMessage"].as_str().map(str::to_owned);
                    return Ok(());
                }
                _ => return Err("Scheduled agent execution failed"),
            }
        }
    }
}
