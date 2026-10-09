//! Workspace list snapshots, synchronization and subscription projections.

use std::collections::{BTreeMap, BTreeSet};

use domain::directory_sync::{Cursor, Mode};
use domain::workspace::protocol::projection::{project_descriptor, workspace_descriptor};
use model::ServerMessage;
use serde_json::{Value, json};

use super::{
    Directory, ErrorCode, WorkspaceDescriptorPayload, WorkspaceListRequest, WorkspaceListResult,
    WorkspaceProjectDescriptorPayload, active_project, active_workspace, directory_error, encode,
    matches_filter, pagination, project_matches_filter, timestamp,
};

struct Snapshot {
    entries: Vec<WorkspaceDescriptorPayload>,
    empty: Vec<WorkspaceProjectDescriptorPayload>,
    projects: BTreeSet<String>,
}

pub(crate) struct Observation {
    id: String,
    request: WorkspaceListRequest,
    previous: BTreeMap<String, Value>,
    empty_projects: BTreeSet<String>,
    paths: Vec<String>,
    git_observer: Option<std::sync::Arc<dyn model::workspace::git::WorkspaceGitObserver>>,
    git_interest: Option<Box<dyn model::workspace::git::WorkspaceGitObservation>>,
}

pub(super) fn list(
    directory: &Directory,
    request: &WorkspaceListRequest,
) -> Result<Value, ErrorCode> {
    response(directory, request, snapshot(directory, request)?)
}

pub(crate) fn prepare(
    directory: &Directory,
    mut request: WorkspaceListRequest,
    id: String,
) -> Result<(Value, Observation), ErrorCode> {
    if request
        .subscribe
        .as_ref()
        .is_some_and(|subscribe| subscribe.subscription_id.is_some())
    {
        return Err(ErrorCode::InvalidMessage);
    }
    request.subscribe = None;
    let snapshot = snapshot(directory, &request)?;
    let previous = wire_entries(&snapshot.entries)?;
    let paths = snapshot
        .entries
        .iter()
        .map(|entry| entry.workspace_directory.clone())
        .collect();
    let empty_projects = snapshot
        .empty
        .iter()
        .map(|project| project.project_id.clone())
        .collect();
    let mut value = response(directory, &request, snapshot)?;
    if request.sync.is_some() {
        request.sync = Some(checkpoint(&value)?);
    }
    value["subscriptionId"] = json!(id);
    Ok((
        value,
        Observation {
            id,
            request,
            previous,
            empty_projects,
            paths,
            git_observer: directory.git_observer(),
            git_interest: None,
        },
    ))
}

impl Observation {
    pub(crate) fn activate(&mut self) {
        if let Some(observer) = &self.git_observer {
            let mut observation = observer.observe();
            observation.set_paths(&self.paths);
            self.git_interest = Some(observation);
        }
    }

    pub(crate) fn update(
        &mut self,
        directory: &Directory,
    ) -> Result<Vec<ServerMessage>, ErrorCode> {
        let snapshot = snapshot(directory, &self.request)?;
        if let Some(observation) = &mut self.git_interest {
            self.paths = snapshot
                .entries
                .iter()
                .map(|entry| entry.workspace_directory.clone())
                .collect();
            observation.set_paths(&self.paths);
        }
        let next = wire_entries(&snapshot.entries)?;
        let empty: BTreeMap<_, _> = snapshot
            .empty
            .iter()
            .map(|project| Ok((project.project_id.clone(), encode(project)?)))
            .collect::<Result<_, ErrorCode>>()?;
        let mut updates = Vec::new();
        if let Some(cursor) = &self.request.sync {
            let read = directory
                .directory_sync()
                .synchronize("workspaces", next.clone(), cursor);
            if read.sync.mode != Mode::Changes {
                return Err(ErrorCode::RegistryIo);
            }
            for mut workspace in read.values {
                let seq = workspace
                    .as_object_mut()
                    .and_then(|value| value.remove("syncSeq"));
                updates.push(json!({"kind":"upsert","workspace":workspace,"generation":read.sync.generation,"seq":seq}));
            }
            for removal in &read.sync.removals {
                let mut update = remove(
                    &removal.id,
                    self.previous.get(&removal.id),
                    &empty,
                    &snapshot.projects,
                );
                update["generation"] = json!(read.sync.generation);
                update["seq"] = json!(removal.seq);
                updates.push(update);
            }
            self.request.sync = Some(Cursor {
                generation: Some(read.sync.generation),
                after_seq: Some(read.sync.head_seq),
            });
            updates.sort_unstable_by_key(|update| update["seq"].as_u64().unwrap_or_default());
        } else {
            for (id, workspace) in &next {
                if self.previous.get(id) != Some(workspace) {
                    updates.push(json!({"kind":"upsert","workspace":workspace}));
                }
            }
            for (id, previous) in &self.previous {
                if !next.contains_key(id) {
                    updates.push(remove(id, Some(previous), &empty, &snapshot.projects));
                }
            }
            for id in self.empty_projects.difference(&snapshot.projects) {
                updates.push(json!({"kind":"remove","id":id,"removedProjectId":id}));
            }
        }
        self.previous = next;
        self.empty_projects = empty.into_keys().collect();
        Ok(updates
            .into_iter()
            .map(|mut params| {
                params["subscriptionId"] = json!(self.id);
                ServerMessage::Event {
                    method: "workspace.update".to_owned(),
                    params,
                }
            })
            .collect())
    }
}

fn remove(
    id: &str,
    previous: Option<&Value>,
    empty: &BTreeMap<String, Value>,
    projects: &BTreeSet<String>,
) -> Value {
    let mut update = json!({"kind":"remove","id":id});
    if let Some(project) = previous.and_then(|row| row["projectId"].as_str()) {
        if let Some(empty) = empty.get(project) {
            update["emptyProject"] = empty.clone();
        }
        if !projects.contains(project) {
            update["removedProjectId"] = json!(project);
        }
    }
    update
}

fn wire_entries(
    entries: &[WorkspaceDescriptorPayload],
) -> Result<BTreeMap<String, Value>, ErrorCode> {
    entries
        .iter()
        .map(|entry| Ok((entry.id.clone(), encode(entry)?)))
        .collect()
}

fn snapshot(directory: &Directory, request: &WorkspaceListRequest) -> Result<Snapshot, ErrorCode> {
    if request.sync.is_some() && request.filter.is_some()
        || request
            .page
            .as_ref()
            .is_some_and(|page| !(1..=200).contains(&page.limit))
    {
        return Err(ErrorCode::InvalidMessage);
    }
    let projects: BTreeMap<_, _> = directory
        .list_projects()
        .map_err(directory_error)?
        .into_iter()
        .map(|project| (project.project_id.clone(), project))
        .collect();
    let all_active: Vec<_> = directory
        .list_workspaces()
        .map_err(directory_error)?
        .into_iter()
        .filter(active_workspace)
        .filter(|workspace| {
            projects
                .get(&workspace.project_id)
                .is_none_or(active_project)
        })
        .collect();
    let statuses = directory
        .workspace_statuses(&all_active, &timestamp())
        .map_err(directory_error)?;
    let mut runtimes = BTreeMap::new();
    let entries = all_active
        .iter()
        .filter(|workspace| matches_filter(workspace, request))
        .map(|workspace| {
            let mut descriptor =
                workspace_descriptor(workspace, projects.get(&workspace.project_id));
            if let Some(runtime) = runtimes
                .entry(workspace.cwd.as_str())
                .or_insert_with(|| directory.runtime_snapshot(&workspace.cwd))
            {
                super::runtime::apply(&mut descriptor, runtime);
            }
            if let Some(status) = statuses.get(&workspace.workspace_id) {
                descriptor.status = status.bucket;
                descriptor.status_entered_at = Some(status.entered_at.clone());
            }
            descriptor
        })
        .collect();
    let occupied: BTreeSet<_> = all_active
        .iter()
        .map(|workspace| workspace.project_id.as_str())
        .collect();
    let empty = projects
        .values()
        .filter(|project| active_project(project))
        .filter(|project| !occupied.contains(project.project_id.as_str()))
        .filter(|project| project_matches_filter(project, request))
        .map(project_descriptor)
        .collect();
    Ok(Snapshot {
        entries,
        empty,
        projects: projects
            .into_values()
            .filter(active_project)
            .map(|project| project.project_id)
            .collect(),
    })
}

fn response(
    directory: &Directory,
    request: &WorkspaceListRequest,
    snapshot: Snapshot,
) -> Result<Value, ErrorCode> {
    if let Some(cursor) = &request.sync {
        let read = directory.directory_sync().synchronize(
            "workspaces",
            wire_entries(&snapshot.entries)?,
            cursor,
        );
        return Ok(json!({"entries":read.values,"emptyProjects":[],
            "pageInfo":{"nextCursor":null,"prevCursor":null,"hasMore":false},"sync":read.sync}));
    }
    let (entries, page_info) = pagination::paginate(
        snapshot.entries,
        request.sort.as_deref(),
        request.page.as_ref(),
    )?;
    let first = request
        .page
        .as_ref()
        .and_then(|page| page.cursor.as_deref())
        .is_none_or(str::is_empty);
    encode(WorkspaceListResult {
        entries,
        page_info,
        empty_projects: if first { snapshot.empty } else { Vec::new() },
    })
}

fn checkpoint(value: &Value) -> Result<Cursor, ErrorCode> {
    Ok(Cursor {
        generation: Some(
            value["sync"]["generation"]
                .as_str()
                .ok_or(ErrorCode::RegistryIo)?
                .to_owned(),
        ),
        after_seq: Some(
            value["sync"]["headSeq"]
                .as_u64()
                .ok_or(ErrorCode::RegistryIo)?,
        ),
    })
}
