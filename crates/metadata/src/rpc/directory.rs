//! Project and Workspace request handling and descriptor projections.

/// Client methods implemented by this component branch.
pub const METHODS: &[MethodSpec] = &[
    MethodSpec::request("project.add.request"),
    MethodSpec::request("project.create_directory.request"),
    MethodSpec::request("project.list.request"),
    MethodSpec::request("project.rename.request"),
    MethodSpec::request("project.remove.request"),
    MethodSpec::request("workspace.open.request"),
    MethodSpec::request("workspace.create.request"),
    MethodSpec::request("workspace.list.request"),
    MethodSpec::request("workspace.archive.request"),
    MethodSpec::request("workspace.title.set.request"),
    MethodSpec::request("workspace.pin.set.request"),
];

/// Client methods implemented by this component branch.
pub const PROJECT_CONFIG_METHODS: &[MethodSpec] = &[
    MethodSpec::request("project.config.read.request"),
    MethodSpec::request("project.config.write.request"),
];

/// Client methods implemented by this component branch.
pub const PROJECT_ICON_METHODS: &[MethodSpec] = &[
    MethodSpec::request("project.icon.set.request"),
    MethodSpec::request("project.icon.get.request"),
];

use base64::Engine;
use chrono::{SecondsFormat, Utc};
use model::methods::MethodSpec;
use model::workspace::protocol::directory::{
    ProjectAddRequest, ProjectAddResult, ProjectCreateDirectoryRequest,
    ProjectCreateDirectoryResult, ProjectListRequest, ProjectListResult, ProjectRemoveRequest,
    ProjectRemoveResult, ProjectRenameRequest, ProjectRenameResult, WorkspaceArchiveRequest,
    WorkspaceArchiveResult, WorkspaceCreateRequest, WorkspaceCreateResult, WorkspaceCreateSource,
    WorkspaceListRequest, WorkspaceListResult, WorkspaceOpenRequest, WorkspaceOpenResult,
    WorkspacePinSetRequest, WorkspacePinSetResult, WorkspaceTitleSetRequest,
    WorkspaceTitleSetResult,
};
use model::workspace::protocol::projection::{project_descriptor, workspace_descriptor};
use model::workspace::protocol::workspace::{
    WorkspaceDescriptorPayload, WorkspaceProjectDescriptorPayload,
};
use model::workspace::records::{PersistedProjectRecord, PersistedWorkspaceRecord};
use serde::Serialize;
use serde_json::Value;

use crate::protocol::project_config::{
    PaseoConfigRaw, PaseoConfigRevision, ProjectConfigReadRequest, ProjectConfigReadResult,
    ProjectConfigRpcError, ProjectConfigWriteRequest, ProjectConfigWriteResult,
};
use crate::protocol::project_icon::{
    ProjectIconGetRequest, ProjectIconGetResult, ProjectIconPayload, ProjectIconSetRequest,
    ProjectIconSetResult, ProjectIconSource,
};
use crate::rpc::ErrorCode;
use crate::service::directory::{Directory, DirectoryError};

pub(crate) mod listing;
mod pagination;
mod runtime;

/// Decode and execute one metadata directory request.
///
/// # Errors
/// Returns safe validation, capability or registry errors; expected business failures stay inline.
pub fn execute(directory: &mut Directory, method: &str, params: Value) -> Result<Value, ErrorCode> {
    match method {
        "project.add.request" => project_add(directory, &decode(params)?),
        "project.create_directory.request" => project_create_directory(directory, &decode(params)?),
        "project.config.read.request" => project_config_read(directory, decode(params)?),
        "project.config.write.request" => project_config_write(directory, decode(params)?),
        "project.icon.set.request" => project_icon_set(directory, decode(params)?),
        "project.icon.get.request" => project_icon_get(directory, decode(params)?),
        "project.list.request" => project_list(directory, &decode(params)?),
        "project.rename.request" => project_rename(directory, decode(params)?),
        "project.remove.request" => project_remove(directory, decode(params)?),
        "workspace.open.request" => workspace_open(directory, &decode(params)?),
        "workspace.create.request" => {
            workspace_creation(directory, params).map(|reply| reply.value)
        }
        "workspace.list.request" => workspace_list(directory, &decode(params)?),
        "workspace.archive.request" => workspace_archive(directory, decode(params)?),
        "workspace.title.set.request" => workspace_title_set(directory, decode(params)?),
        "workspace.pin.set.request" => workspace_pin_set(directory, decode(params)?),
        _ => Err(ErrorCode::MethodNotFound),
    }
}

fn project_icon_set(
    directory: &Directory,
    request: ProjectIconSetRequest,
) -> Result<Value, ErrorCode> {
    let upload = match request.source {
        ProjectIconSource::Automatic => None,
        ProjectIconSource::Upload { data } => {
            let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) else {
                return encode(ProjectIconSetResult {
                    project_id: request.project_id,
                    accepted: false,
                    error: Some("Unsupported or invalid icon file".to_owned()),
                });
            };
            Some(bytes)
        }
    };
    let timestamp = timestamp();
    match directory.set_project_icon(&request.project_id, upload.as_deref(), &timestamp) {
        Ok(Some(_)) => encode(ProjectIconSetResult {
            project_id: request.project_id,
            accepted: true,
            error: None,
        }),
        Ok(None) | Err(DirectoryError::UnknownProject) => encode(ProjectIconSetResult {
            project_id: request.project_id,
            accepted: false,
            error: Some("Project not found".to_owned()),
        }),
        Err(DirectoryError::Registry) => Err(ErrorCode::RegistryIo),
        Err(error) => encode(ProjectIconSetResult {
            project_id: request.project_id,
            accepted: false,
            error: Some(error.to_string()),
        }),
    }
}

fn project_icon_get(
    directory: &Directory,
    request: ProjectIconGetRequest,
) -> Result<Value, ErrorCode> {
    match directory.get_project_icon(&request.project_id) {
        Ok(icon) => encode(ProjectIconGetResult {
            project_id: request.project_id,
            icon: icon.map(|icon| ProjectIconPayload {
                data: base64::engine::general_purpose::STANDARD.encode(icon.bytes),
                mime_type: icon.mime_type,
            }),
            error: None,
        }),
        Err(DirectoryError::UnknownProject) => encode(ProjectIconGetResult {
            project_id: request.project_id,
            icon: None,
            error: Some("Project not found".to_owned()),
        }),
        Err(DirectoryError::Registry) => Err(ErrorCode::RegistryIo),
        Err(error) => encode(ProjectIconGetResult {
            project_id: request.project_id,
            icon: None,
            error: Some(error.to_string()),
        }),
    }
}

fn project_config_read(
    directory: &Directory,
    request: ProjectConfigReadRequest,
) -> Result<Value, ErrorCode> {
    match directory.read_project_config(&request.repo_root) {
        Ok(result) => {
            let config = match result.config {
                Some(config) => match PaseoConfigRaw::new(config) {
                    Ok(config) => Some(config),
                    Err(_) => {
                        return encode(ProjectConfigReadResult::Failure {
                            repo_root: result.repo_root,
                            error: ProjectConfigRpcError::InvalidProjectConfig,
                        });
                    }
                },
                None => None,
            };
            encode(ProjectConfigReadResult::Success {
                repo_root: result.repo_root,
                config,
                revision: result.revision.map(protocol_revision),
            })
        }
        Err(DirectoryError::UnknownProject) => encode(ProjectConfigReadResult::Failure {
            repo_root: request.repo_root,
            error: ProjectConfigRpcError::ProjectNotFound,
        }),
        Err(DirectoryError::Registry) => Err(ErrorCode::RegistryIo),
        Err(
            DirectoryError::DirectoryNotFound
            | DirectoryError::InvalidDirectoryName
            | DirectoryError::ParentDirectoryNotFound
            | DirectoryError::DirectoryExists
            | DirectoryError::WorkspaceIdConflict
            | DirectoryError::PermissionDenied
            | DirectoryError::FileSystem
            | DirectoryError::RegistrationFailed { .. }
            | DirectoryError::ArchivedProject
            | DirectoryError::InvalidProjectConfig
            | DirectoryError::StaleProjectConfig { .. }
            | DirectoryError::ProjectConfigWriteFailed
            | DirectoryError::InvalidProjectIcon
            | DirectoryError::ProjectIconStorage,
        ) => encode(ProjectConfigReadResult::Failure {
            repo_root: request.repo_root,
            error: ProjectConfigRpcError::InvalidProjectConfig,
        }),
    }
}

fn project_config_write(
    directory: &Directory,
    request: ProjectConfigWriteRequest,
) -> Result<Value, ErrorCode> {
    let expected_revision = request.expected_revision.map(port_revision);
    match directory.write_project_config(
        &request.repo_root,
        request.config.value(),
        expected_revision,
    ) {
        Ok(result) => encode(ProjectConfigWriteResult::Success {
            repo_root: result.repo_root,
            config: PaseoConfigRaw::new(result.config).map_err(|_| ErrorCode::RegistryIo)?,
            revision: protocol_revision(result.revision),
        }),
        Err(DirectoryError::UnknownProject) => encode(ProjectConfigWriteResult::Failure {
            repo_root: request.repo_root,
            error: ProjectConfigRpcError::ProjectNotFound,
        }),
        Err(DirectoryError::StaleProjectConfig { current_revision }) => {
            encode(ProjectConfigWriteResult::Failure {
                repo_root: request.repo_root,
                error: ProjectConfigRpcError::StaleProjectConfig {
                    current_revision: current_revision.map(protocol_revision),
                },
            })
        }
        Err(DirectoryError::InvalidProjectConfig) => encode(ProjectConfigWriteResult::Failure {
            repo_root: request.repo_root,
            error: ProjectConfigRpcError::InvalidProjectConfig,
        }),
        Err(DirectoryError::ProjectConfigWriteFailed) => {
            encode(ProjectConfigWriteResult::Failure {
                repo_root: request.repo_root,
                error: ProjectConfigRpcError::WriteFailed,
            })
        }
        Err(DirectoryError::Registry) => Err(ErrorCode::RegistryIo),
        Err(_) => encode(ProjectConfigWriteResult::Failure {
            repo_root: request.repo_root,
            error: ProjectConfigRpcError::WriteFailed,
        }),
    }
}

fn project_add(directory: &Directory, request: &ProjectAddRequest) -> Result<Value, ErrorCode> {
    let timestamp = timestamp();
    match directory.add_project(&request.cwd, &timestamp) {
        Ok(project) => encode(ProjectAddResult {
            project: Some(project_descriptor(&project)),
            error: None,
            error_code: None,
        }),
        Err(DirectoryError::DirectoryNotFound) => encode(ProjectAddResult {
            project: None,
            error: Some(format!("Directory not found: {}", request.cwd)),
            error_code: Some("directory_not_found".to_owned()),
        }),
        Err(DirectoryError::Registry) => Err(ErrorCode::RegistryIo),
        Err(error) => encode(ProjectAddResult {
            project: None,
            error: Some(error.to_string()),
            error_code: None,
        }),
    }
}

fn project_create_directory(
    directory: &Directory,
    request: &ProjectCreateDirectoryRequest,
) -> Result<Value, ErrorCode> {
    let timestamp = timestamp();
    match directory.create_project_directory(&request.parent_path, &request.name, &timestamp) {
        Ok((directory_path, project)) => encode(ProjectCreateDirectoryResult {
            directory_path: Some(directory_path),
            project: Some(project_descriptor(&project)),
            error: None,
            error_code: None,
        }),
        Err(DirectoryError::Registry) => Err(ErrorCode::RegistryIo),
        Err(error) => {
            let directory_path = match &error {
                DirectoryError::RegistrationFailed { directory_path, .. } => {
                    Some(directory_path.clone())
                }
                _ => None,
            };
            encode(ProjectCreateDirectoryResult {
                directory_path,
                project: None,
                error: Some(error.to_string()),
                error_code: Some(directory_create_error_code(&error).to_owned()),
            })
        }
    }
}

fn workspace_open(
    directory: &Directory,
    request: &WorkspaceOpenRequest,
) -> Result<Value, ErrorCode> {
    let timestamp = timestamp();
    match directory.open_workspace(&request.cwd, &timestamp) {
        Ok(workspace) => encode(WorkspaceOpenResult {
            workspace: Some(describe_workspace(directory, &workspace)?),
            error: None,
            error_code: None,
        }),
        Err(DirectoryError::DirectoryNotFound) => encode(WorkspaceOpenResult {
            workspace: None,
            error: Some(format!("Directory not found: {}", request.cwd)),
            error_code: Some("directory_not_found".to_owned()),
        }),
        Err(DirectoryError::Registry) => Err(ErrorCode::RegistryIo),
        Err(error) => encode(WorkspaceOpenResult {
            workspace: None,
            error: Some(error.to_string()),
            error_code: None,
        }),
    }
}

/// Workspace creation reply and effects to run only for a fresh worktree creation.
#[derive(Debug)]
pub struct WorkspaceCreated {
    /// Response including the durable creation receipt.
    pub value: Value,
    /// Newly created worktree whose setup and update should be dispatched.
    pub created_worktree_id: Option<String>,
    /// Fresh Workspace receipt awaiting its initial Agent, owned by the API coordinator.
    pub pending_agent: Option<model::creation::protocol::Snapshot>,
}

/// Create or replay a Workspace intent using the metadata creation coordinator.
///
/// # Errors
/// Returns validation, unsupported service, receipt conflict, or persistence errors.
pub fn workspace_creation(
    directory: &Directory,
    params: Value,
) -> Result<WorkspaceCreated, ErrorCode> {
    create_workspace_intent(directory, params, None)
}

/// Provision a Workspace with a validated, secret-free initial Agent receipt intent.
/// The API coordinator owns Agent validation and must finish the returned pending receipt.
/// # Errors
/// Returns invalid source, missing provisioning, idempotency, or persistence failures.
pub fn workspace_creation_with_agent(
    directory: &Directory,
    params: Value,
    agent_intent: Value,
) -> Result<WorkspaceCreated, ErrorCode> {
    create_workspace_intent(directory, params, Some(agent_intent))
}

fn create_workspace_intent(
    directory: &Directory,
    mut params: Value,
    agent_intent: Option<Value>,
) -> Result<WorkspaceCreated, ErrorCode> {
    use model::creation::protocol::Kind;
    let mut request: WorkspaceCreateRequest = decode(params.clone())?;
    let is_worktree = matches!(request.source, WorkspaceCreateSource::Worktree(_));
    if (request.agent.is_some() && agent_intent.is_none())
        || (is_worktree && directory.worktrees().is_none())
    {
        return Err(ErrorCode::UnsupportedCapability);
    }
    let has_agent = agent_intent.is_some();
    if let Some(intent) = agent_intent {
        request.first_agent_context =
            Some(model::workspace::protocol::directory::FirstAgentContext {
                prompt: intent["initialPrompt"].as_str().map(str::to_owned),
                attachments: intent["attachments"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default(),
            });
        request.agent = None;
        params["agent"] = intent;
    }
    let key = request
        .idempotency_key
        .take()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if let Some(object) = params.as_object_mut() {
        object.remove("idempotencyKey");
        object.remove("subscribe");
    }
    let creations = directory.creations();
    let admission = creations.begin(Kind::Workspace, &key, params)?;
    if admission.snapshot.phase == "completed"
        && let Some(id) = &admission.snapshot.workspace_id
        && !directory
            .contains_workspace_directory(id)
            .map_err(|_| ErrorCode::RegistryIo)?
    {
        return Err(ErrorCode::WorkspaceNotFound);
    }
    if !admission.execute || (has_agent && admission.snapshot.workspace.is_some()) {
        return Ok(WorkspaceCreated {
            value: serde_json::json!({"workspace":admission.snapshot.workspace,"agent":admission.snapshot.agent,"setupTerminalId":null,"error":admission.snapshot.error,"creation":admission.snapshot}),
            created_worktree_id: None,
            pending_agent: admission.execute.then_some(admission.snapshot),
        });
    }
    request
        .workspace_id
        .clone_from(&admission.snapshot.workspace_id);
    let result = workspace_create(directory, request);
    match result {
        Ok(mut value) => {
            let error = value["error"].as_str().map(str::to_owned);
            let progress = if error.is_some() {
                creations.advance(&admission.snapshot, "failed", Some(value.clone()), error)?
            } else {
                let ready = creations.advance(
                    &admission.snapshot,
                    "workspace_ready",
                    Some(value.clone()),
                    None,
                )?;
                if has_agent {
                    ready
                } else {
                    creations.advance(&ready, "completed", None, None)?
                }
            };
            let pending_agent =
                (has_agent && progress.phase == "workspace_ready").then(|| progress.clone());
            value["creation"] =
                serde_json::to_value(progress).map_err(|_| ErrorCode::RegistryIo)?;
            let created_worktree_id = is_worktree
                .then(|| value["workspace"]["id"].as_str().map(str::to_owned))
                .flatten();
            Ok(WorkspaceCreated {
                value,
                created_worktree_id,
                pending_agent,
            })
        }
        Err(error) => {
            creations.advance(
                &admission.snapshot,
                "failed",
                None,
                Some(model::ErrorCode::from(error).message().to_owned()),
            )?;
            Err(error)
        }
    }
}

fn workspace_create(
    directory: &Directory,
    request: WorkspaceCreateRequest,
) -> Result<Value, ErrorCode> {
    if request.agent.is_some() {
        return Err(ErrorCode::UnsupportedCapability);
    }
    let (path, project_id) = match request.source.clone() {
        WorkspaceCreateSource::Directory { path, project_id } => (path, project_id),
        WorkspaceCreateSource::Worktree(source) => {
            return workspace_create_worktree(directory, request, source);
        }
    };
    let timestamp = timestamp();
    match directory.create_workspace(model::workspace::lifecycle::WorkspaceCreation {
        path: &path,
        title: request.title,
        project_id: project_id.as_deref(),
        workspace_id: request.workspace_id,
        expects_initial_agent: request.first_agent_context.is_some(),
        timestamp: &timestamp,
    }) {
        Ok(workspace) => {
            if let Some(context) = request.first_agent_context
                && let Some(source) = model::workspace::naming::first_agent_source(
                    context.prompt.as_deref(),
                    &context.attachments,
                )
            {
                directory.name_workspace(workspace.workspace_id.clone(), source);
            }
            encode(WorkspaceCreateResult {
                workspace: Some(describe_workspace(directory, &workspace)?),
                setup_terminal_id: None,
                error: None,
                error_code: None,
            })
        }
        Err(DirectoryError::Registry) => Err(ErrorCode::RegistryIo),
        Err(error) => encode(WorkspaceCreateResult {
            workspace: None,
            setup_terminal_id: None,
            error: Some(error.to_string()),
            error_code: workspace_create_error_code(&error).map(str::to_owned),
        }),
    }
}

fn workspace_create_worktree(
    directory: &Directory,
    request: WorkspaceCreateRequest,
    source: model::workspace::protocol::directory::WorkspaceWorktreeSource,
) -> Result<Value, ErrorCode> {
    use model::workspace::protocol::directory::WorkspaceWorktreeAction;
    use model::workspace::worktrees::{WorktreeAction, WorktreeCreation};
    let provisioning = directory
        .worktrees()
        .ok_or(ErrorCode::UnsupportedCapability)?;
    let result = provisioning.create(
        &WorktreeCreation {
            cwd: source.cwd,
            project_id: source.project_id,
            workspace_id: request.workspace_id,
            title: request.title,
            worktree_slug: source.worktree_slug,
            ref_name: source.ref_name,
            base_branch: source.base_branch,
            branch_name: source.branch_name,
            action: match source.action.unwrap_or(WorkspaceWorktreeAction::BranchOff) {
                WorkspaceWorktreeAction::BranchOff => WorktreeAction::BranchOff,
                WorkspaceWorktreeAction::Checkout => WorktreeAction::Checkout,
            },
            checkout_source: source
                .checkout_source
                .map(model::workspace::protocol::worktree_source::ChangeRequestCheckoutSource::into_intent)
                .or_else(|| {
                    source.github_pr_number.map(|number| {
                        model::workspace::worktrees::WorktreeChangeRequest {
                            forge: Some("github".to_owned()),
                            number: number.get(),
                            project_path: None,
                        }
                    })
                }),
            first_agent_prompt: request
                .first_agent_context
                .as_ref()
                .and_then(|context| context.prompt.clone()),
            expects_initial_agent: request.first_agent_context.is_some(),
        },
        &timestamp(),
    );
    encode(match result {
        Ok(created) => {
            if let Some(context) = request.first_agent_context
                && let Some(source) = model::workspace::naming::first_agent_source(
                    context.prompt.as_deref(),
                    &context.attachments,
                )
            {
                directory.name_workspace(created.workspace.workspace_id.clone(), source);
            }
            WorkspaceCreateResult {
                workspace: Some(runtime_workspace_descriptor(
                    directory,
                    &created.workspace,
                    Some(&created.project),
                )),
                setup_terminal_id: None,
                error: None,
                error_code: None,
            }
        }
        Err(error) => WorkspaceCreateResult {
            workspace: None,
            setup_terminal_id: None,
            error_code: Some(error.code.to_owned()),
            error: Some(error.message),
        },
    })
}

fn project_list(directory: &Directory, request: &ProjectListRequest) -> Result<Value, ErrorCode> {
    let projects = directory
        .list_projects()
        .map_err(directory_error)?
        .into_iter()
        .filter(active_project)
        .map(|project| project_descriptor(&project))
        .collect();
    if let Some(cursor) = &request.sync {
        let value = encode(ProjectListResult { projects })?;
        let rows = value["projects"].as_array().ok_or(ErrorCode::RegistryIo)?;
        let read = directory.directory_sync().synchronize(
            "projects",
            rows.iter().map(|row| {
                (
                    row["projectId"].as_str().unwrap_or_default().to_owned(),
                    row.clone(),
                )
            }),
            cursor,
        );
        return Ok(serde_json::json!({"projects":read.values,"sync":read.sync}));
    }
    encode(ProjectListResult { projects })
}

fn project_rename(
    directory: &Directory,
    request: ProjectRenameRequest,
) -> Result<Value, ErrorCode> {
    let custom_name = normalize_optional_text(request.custom_name);
    let timestamp = timestamp();
    let updated = directory
        .rename_project(&request.project_id, custom_name.as_deref(), &timestamp)
        .map_err(directory_error)?;
    encode(match updated {
        Some(_) => ProjectRenameResult {
            project_id: request.project_id,
            accepted: true,
            custom_name,
            error: None,
        },
        None => ProjectRenameResult {
            project_id: request.project_id,
            accepted: false,
            custom_name: None,
            error: Some("Project not found".to_owned()),
        },
    })
}

fn project_remove(
    directory: &Directory,
    request: ProjectRemoveRequest,
) -> Result<Value, ErrorCode> {
    let timestamp = timestamp();
    let active_workspace_ids = directory
        .remove_project(&request.project_id, &timestamp)
        .map_err(directory_error)?;
    encode(ProjectRemoveResult {
        project_id: request.project_id,
        accepted: true,
        removed_workspace_ids: active_workspace_ids,
        error: None,
    })
}

fn workspace_list(
    directory: &Directory,
    request: &WorkspaceListRequest,
) -> Result<Value, ErrorCode> {
    if request.subscribe.is_some() {
        return Err(ErrorCode::UnsupportedCapability);
    }
    listing::list(directory, request)
}

fn workspace_archive(
    directory: &Directory,
    request: WorkspaceArchiveRequest,
) -> Result<Value, ErrorCode> {
    let archived_at = timestamp();
    if directory
        .archive_workspace(&request.workspace_id, &archived_at)
        .map_err(directory_error)?
        .is_none()
    {
        return encode(WorkspaceArchiveResult {
            workspace_id: request.workspace_id.clone(),
            archived_at: None,
            error: Some(format!("Workspace not found: {}", request.workspace_id)),
        });
    }
    encode(WorkspaceArchiveResult {
        workspace_id: request.workspace_id,
        archived_at: Some(archived_at),
        error: None,
    })
}

fn workspace_title_set(
    directory: &Directory,
    request: WorkspaceTitleSetRequest,
) -> Result<Value, ErrorCode> {
    let title = normalize_optional_text(request.title);
    let timestamp = timestamp();
    let updated = directory
        .set_workspace_title(&request.workspace_id, title.as_deref(), &timestamp)
        .map_err(directory_error)?;
    encode(match updated {
        Some(_) => WorkspaceTitleSetResult {
            workspace_id: request.workspace_id,
            accepted: true,
            title,
            error: None,
        },
        None => WorkspaceTitleSetResult {
            workspace_id: request.workspace_id,
            accepted: false,
            title: None,
            error: Some("Workspace not found".to_owned()),
        },
    })
}

fn workspace_pin_set(
    directory: &Directory,
    request: WorkspacePinSetRequest,
) -> Result<Value, ErrorCode> {
    let timestamp = timestamp();
    let pinned_at = request.pinned.then(|| timestamp.clone());
    let updated = directory
        .set_workspace_pin(&request.workspace_id, pinned_at.as_deref(), &timestamp)
        .map_err(directory_error)?;
    encode(match updated {
        Some(_) => WorkspacePinSetResult {
            workspace_id: request.workspace_id,
            accepted: true,
            pinned_at,
            error: None,
        },
        None => WorkspacePinSetResult {
            workspace_id: request.workspace_id,
            accepted: false,
            pinned_at: None,
            error: Some("Workspace not found".to_owned()),
        },
    })
}

fn describe_workspace(
    directory: &Directory,
    workspace: &PersistedWorkspaceRecord,
) -> Result<WorkspaceDescriptorPayload, ErrorCode> {
    let project = directory
        .list_projects()
        .map_err(directory_error)?
        .into_iter()
        .find(|project| project.project_id == workspace.project_id);
    Ok(runtime_workspace_descriptor(
        directory,
        workspace,
        project.as_ref(),
    ))
}

fn runtime_workspace_descriptor(
    directory: &Directory,
    workspace: &PersistedWorkspaceRecord,
    project: Option<&PersistedProjectRecord>,
) -> WorkspaceDescriptorPayload {
    let mut descriptor = workspace_descriptor(workspace, project);
    if let Some(snapshot) = directory.runtime_snapshot(&workspace.cwd) {
        runtime::apply(&mut descriptor, &snapshot);
    }
    descriptor
}

fn matches_filter(workspace: &PersistedWorkspaceRecord, request: &WorkspaceListRequest) -> bool {
    let Some(filter) = &request.filter else {
        return true;
    };
    if filter
        .project_id
        .as_deref()
        .map(str::trim)
        .filter(|project_id| !project_id.is_empty())
        .is_some_and(|project_id| project_id != workspace.project_id)
    {
        return false;
    }
    let Some(query) = filter
        .query
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
    else {
        return true;
    };
    let query = query.to_lowercase();
    [
        workspace.display_name(),
        &workspace.project_id,
        &workspace.workspace_id,
    ]
    .into_iter()
    .any(|candidate| candidate.to_lowercase().contains(&query))
}

fn project_matches_filter(
    project: &PersistedProjectRecord,
    request: &WorkspaceListRequest,
) -> bool {
    let Some(filter) = &request.filter else {
        return true;
    };
    filter
        .project_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .is_none_or(|id| id == project.project_id)
}

fn active_project(project: &PersistedProjectRecord) -> bool {
    project.archived_at.as_ref().is_none_or(String::is_empty)
}

fn active_workspace(workspace: &PersistedWorkspaceRecord) -> bool {
    workspace.archived_at.as_ref().is_none_or(String::is_empty)
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

const fn directory_create_error_code(error: &DirectoryError) -> &'static str {
    match error {
        DirectoryError::InvalidDirectoryName => "invalid_name",
        DirectoryError::ParentDirectoryNotFound | DirectoryError::DirectoryNotFound => {
            "parent_directory_not_found"
        }
        DirectoryError::DirectoryExists => "directory_exists",
        DirectoryError::PermissionDenied => "permission_denied",
        DirectoryError::RegistrationFailed { .. }
        | DirectoryError::WorkspaceIdConflict
        | DirectoryError::UnknownProject
        | DirectoryError::ArchivedProject => "registration_failed",
        DirectoryError::FileSystem
        | DirectoryError::Registry
        | DirectoryError::InvalidProjectConfig
        | DirectoryError::StaleProjectConfig { .. }
        | DirectoryError::ProjectConfigWriteFailed
        | DirectoryError::InvalidProjectIcon
        | DirectoryError::ProjectIconStorage => "filesystem_error",
    }
}

const fn workspace_create_error_code(error: &DirectoryError) -> Option<&'static str> {
    match error {
        DirectoryError::DirectoryNotFound => Some("directory_not_found"),
        DirectoryError::UnknownProject => Some("unknown_project"),
        DirectoryError::ArchivedProject => Some("archived_project"),
        DirectoryError::WorkspaceIdConflict => Some("workspace_id_conflict"),
        DirectoryError::InvalidDirectoryName
        | DirectoryError::ParentDirectoryNotFound
        | DirectoryError::DirectoryExists
        | DirectoryError::PermissionDenied
        | DirectoryError::FileSystem
        | DirectoryError::RegistrationFailed { .. }
        | DirectoryError::Registry
        | DirectoryError::InvalidProjectConfig
        | DirectoryError::StaleProjectConfig { .. }
        | DirectoryError::ProjectConfigWriteFailed
        | DirectoryError::InvalidProjectIcon
        | DirectoryError::ProjectIconStorage => None,
    }
}

const fn protocol_revision(
    revision: crate::service::directory::ProjectConfigRevision,
) -> PaseoConfigRevision {
    PaseoConfigRevision {
        mtime_ms: revision.mtime_ms,
        size: revision.size,
    }
}

const fn port_revision(
    revision: PaseoConfigRevision,
) -> crate::service::directory::ProjectConfigRevision {
    crate::service::directory::ProjectConfigRevision {
        mtime_ms: revision.mtime_ms,
        size: revision.size,
    }
}

fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, ErrorCode> {
    serde_json::from_value(value).map_err(|_| ErrorCode::InvalidMessage)
}

fn encode(value: impl Serialize) -> Result<Value, ErrorCode> {
    serde_json::to_value(value).map_err(|_| ErrorCode::RegistryIo)
}

fn directory_error(_error: DirectoryError) -> ErrorCode {
    ErrorCode::RegistryIo
}

#[cfg(test)]
mod tests;
