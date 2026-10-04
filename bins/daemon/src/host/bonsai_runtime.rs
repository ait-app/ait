//! Bonsai runtime ports over the in-process provider worker, timeline and directory.

use std::collections::BTreeSet;
use std::sync::Arc;

use anyhow::Context;
use bonsai::ports::{
    Backfill, Backlog, BoxFuture, Executor, Host, HostEvent, Observation, Observer, PortError,
    Project, Projects, Row, Workspace,
};
use chrono::{SecondsFormat, Utc};
use filesystem::local::provisioning::LocalDirectorySource;
use metadata::ports::provisioning::DirectorySource;
use metadata::service::directory::{Directory, DirectoryError};
use model::ServerMessage;
use model::events::EventHub;
use model::outbound::{Frame, Outbound};
use provider::service::agent_execution::AgentExecution;
use provider::storage::timeline::Timeline;
use serde_json::Value;

/// Agent execution, live events and timeline reads through one provider worker.
#[derive(Debug, Clone)]
struct Agents {
    execution: AgentExecution,
    timeline: Timeline,
}

/// Registered projects and workspaces through the shared directory service.
#[derive(Debug, Clone)]
struct DirectoryProjects {
    directory: Directory,
}

/// Start the Bonsai runtime adapter when `BONSAI_RUNTIME_*` is configured, over the provider
/// worker and directory the server already composed.
pub(super) fn compose(
    config: &crate::config::Config,
    services: &api::Services,
) -> anyhow::Result<Option<bonsai::Service>> {
    let Some(bonsai) = config.bonsai.clone() else {
        return Ok(None);
    };
    let (Some(execution), Some(directory)) =
        (services.agent_execution.clone(), services.directory.clone())
    else {
        anyhow::bail!("the Bonsai runtime needs agent execution and the project directory");
    };
    let data = config
        .data_dir
        .canonicalize()
        .context("resolve Bonsai runtime data directory")?;
    let service = bonsai::Service::spawn(bonsai, ports(execution, directory), &data)
        .context("start the Bonsai runtime adapter")?;
    Ok(Some(service))
}

/// Build the adapter's host ports from the services the server already composed.
pub(super) fn ports(execution: AgentExecution, directory: Directory) -> Host {
    let timeline = execution.timeline();
    let agents = Arc::new(Agents {
        execution,
        timeline,
    });
    Host {
        executor: agents.clone(),
        observer: agents.clone(),
        backfill: agents,
        projects: Arc::new(DirectoryProjects { directory }),
    }
}

impl Executor for Agents {
    fn execute(
        &self,
        method: &'static str,
        params: Value,
    ) -> BoxFuture<'_, Result<Value, model::ErrorCode>> {
        Box::pin(async move {
            self.execution
                .execute(method, params)
                .await
                .map_err(model::ErrorCode::from)
        })
    }
}

impl Observer for Agents {
    fn observe(&self, agent_id: &str) -> Result<Observation, PortError> {
        Ok(observe(&self.timeline.events(), agent_id))
    }
}

/// Observe one Agent's events on a hub, forwarding decoded events until either side closes.
fn observe(hub: &EventHub, agent_id: &str) -> Observation {
    let (outbound, mut queued) = Outbound::new();
    let failure = outbound.failure();
    let subscription = Arc::new(hub.observe(
        format!("bonsai:{agent_id}"),
        BTreeSet::from([agent_id.to_owned()]),
        outbound,
    ));
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let held = Arc::clone(&subscription);
    // Forward without touching the network so AIT's bounded queue never overflows on a slow
    // Bonsai; overflow or shutdown closes the channel and the adapter backfills.
    tokio::spawn(async move {
        let _held = held;
        loop {
            tokio::select! {
                () = sender.closed() => break,
                () = failure.cancelled() => break,
                next = queued.recv() => {
                    let Some(queued) = next else { break };
                    let Frame::Text(text) = queued.message else { continue };
                    if let Ok(ServerMessage::Event { method, params }) = serde_json::from_str(&text)
                        && sender.send(HostEvent { method, params }).is_err()
                    {
                        break;
                    }
                }
            }
        }
    });
    Observation::new(
        receiver,
        Box::new(move || subscription.activate().map_err(|_| PortError::Closed)),
    )
}

impl Backfill for Agents {
    fn read(&self, agent_id: &str) -> BoxFuture<'_, Result<Backlog, PortError>> {
        let timeline = self.timeline.clone();
        let agent = agent_id.to_owned();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || backlog(&timeline, &agent))
                .await
                .map_err(|_| PortError::Storage)?
        })
    }
}

/// Read an Agent's current generation as unmerged rows.
fn backlog(timeline: &Timeline, agent_id: &str) -> Result<Backlog, PortError> {
    let (epoch, rows) = timeline.read(agent_id).map_err(|_| PortError::Storage)?;
    let rows = rows
        .into_iter()
        .map(|row| Row {
            seq: row.seq,
            provider: row.provider,
            turn_id: row.entry.turn_id,
            item: row.entry.item,
        })
        .collect();
    Ok(Backlog { epoch, rows })
}

impl Projects for DirectoryProjects {
    fn list(&self) -> BoxFuture<'_, Result<Vec<Project>, PortError>> {
        let directory = self.directory.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || list_projects(&directory))
                .await
                .map_err(|_| PortError::Storage)?
        })
    }

    fn open_workspace(
        &self,
        project_id: &str,
    ) -> BoxFuture<'_, Result<Option<Workspace>, PortError>> {
        let directory = self.directory.clone();
        let project_id = project_id.to_owned();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || open_workspace(&directory, &project_id))
                .await
                .map_err(|_| PortError::Storage)?
        })
    }
}

fn list_projects(directory: &Directory) -> Result<Vec<Project>, PortError> {
    let records = directory.list_projects().map_err(|_| PortError::Storage)?;
    Ok(records
        .into_iter()
        .filter(|record| record.archived_at.is_none())
        .map(|record| {
            let checkout = LocalDirectorySource.inspect(&record.root_path).ok();
            Project {
                name: record.display_name().to_owned(),
                remote_url: checkout.as_ref().and_then(|c| c.remote_url.clone()),
                branch: checkout.and_then(|c| c.current_branch),
                id: record.project_id,
                root: record.root_path,
            }
        })
        .collect())
}

fn open_workspace(directory: &Directory, project_id: &str) -> Result<Option<Workspace>, PortError> {
    let Some(project) = directory
        .list_projects()
        .map_err(|_| PortError::Storage)?
        .into_iter()
        .find(|record| record.project_id == project_id && record.archived_at.is_none())
    else {
        return Ok(None);
    };
    let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    match directory.open_workspace(&project.root_path, &timestamp) {
        Ok(workspace) => Ok(Some(Workspace {
            workspace_id: workspace.workspace_id,
            cwd: workspace.cwd,
        })),
        Err(
            DirectoryError::DirectoryNotFound
            | DirectoryError::UnknownProject
            | DirectoryError::ArchivedProject,
        ) => Ok(None),
        Err(_) => Err(PortError::Storage),
    }
}

#[cfg(test)]
mod tests;
