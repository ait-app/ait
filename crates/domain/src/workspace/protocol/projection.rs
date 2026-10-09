//! Pure public descriptors for persisted Project and Workspace records.

use std::path::Path;

use super::workspace::{
    ProjectKind, WorkspaceDescriptorPayload, WorkspaceKind, WorkspaceProjectDescriptorPayload,
};
use crate::workspace::activity::WorkspaceStateBucket;
use crate::workspace::records::{
    PersistedProjectKind, PersistedProjectRecord, PersistedWorkspaceKind, PersistedWorkspaceRecord,
};

/// Project a stored Project into its public descriptor.
#[must_use]
pub fn project_descriptor(project: &PersistedProjectRecord) -> WorkspaceProjectDescriptorPayload {
    WorkspaceProjectDescriptorPayload {
        project_id: project.project_id.clone(),
        project_key: project.project_key.clone(),
        project_display_name: project.display_name().to_owned(),
        project_custom_name: project.custom_name.clone(),
        project_custom_icon_revision: project.custom_icon_revision.clone(),
        project_icon_revision: None,
        project_root_path: project.root_path.clone(),
        project_kind: project_kind(project.kind),
        sync_seq: None,
    }
}

/// Project a durable workspace and its optional Project into the public descriptor.
#[must_use]
pub fn workspace_descriptor(
    workspace: &PersistedWorkspaceRecord,
    project: Option<&PersistedProjectRecord>,
) -> WorkspaceDescriptorPayload {
    let project_display_name = project.map_or(workspace.project_id.as_str(), |project| {
        project.display_name()
    });
    let project_root_path = project.map_or_else(
        || {
            workspace
                .main_repo_root
                .clone()
                .unwrap_or_else(|| workspace.cwd.clone())
        },
        |project| project.root_path.clone(),
    );
    WorkspaceDescriptorPayload {
        id: workspace.workspace_id.clone(),
        project_id: workspace.project_id.clone(),
        project_display_name: project_display_name.to_owned(),
        project_custom_name: project.and_then(|project| project.custom_name.clone()),
        project_custom_icon_revision: project
            .and_then(|project| project.custom_icon_revision.clone()),
        project_root_path,
        workspace_directory: workspace.cwd.clone(),
        worktree_slug: workspace
            .is_paseo_owned_worktree
            .then_some(workspace.worktree_root.as_deref())
            .flatten()
            .and_then(|root| Path::new(root).file_name())
            .and_then(|name| name.to_str())
            .map(str::to_owned),
        initial_branch: workspace
            .is_paseo_owned_worktree
            .then(|| workspace.display_name.clone()),
        project_kind: project.map_or_else(
            || match workspace.kind {
                PersistedWorkspaceKind::Directory => ProjectKind::NonGit,
                PersistedWorkspaceKind::LocalCheckout | PersistedWorkspaceKind::Worktree => {
                    ProjectKind::Git
                }
            },
            |project| project_kind(project.kind),
        ),
        workspace_kind: workspace_kind(workspace.kind),
        name: workspace.display_name().to_owned(),
        title: workspace.title.clone(),
        pinned_at: workspace.pinned_at.clone(),
        labels: workspace.labels.clone().filter(|labels| !labels.is_empty()),
        archiving_at: None,
        status: WorkspaceStateBucket::Done,
        status_entered_at: Some(workspace.created_at.clone()),
        activity_at: None,
        diff_stat: None,
        scripts: Vec::new(),
        git_runtime: None,
        github_runtime: None,
        forge: None,
        project: None,
        sync_seq: None,
    }
}

const fn project_kind(kind: PersistedProjectKind) -> ProjectKind {
    match kind {
        PersistedProjectKind::Git => ProjectKind::Git,
        PersistedProjectKind::NonGit => ProjectKind::NonGit,
    }
}

const fn workspace_kind(kind: PersistedWorkspaceKind) -> WorkspaceKind {
    match kind {
        PersistedWorkspaceKind::LocalCheckout => WorkspaceKind::LocalCheckout,
        PersistedWorkspaceKind::Worktree => WorkspaceKind::Worktree,
        PersistedWorkspaceKind::Directory => WorkspaceKind::Directory,
    }
}

#[cfg(test)]
mod tests;
