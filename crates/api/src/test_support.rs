//! Shared fixtures for tests that need real workspaces and share settings.

use std::net::SocketAddr;
use std::sync::Arc;

use filesystem::git::local::provisioning::LocalDirectorySource;
use metadata::directory::service::directory::{Directory, DirectoryDependencies};
use model::share::{Accepted, Outcome, Visitor};
use persistence::storage::project_config::LocalProjectConfigStore;
use persistence::storage::project_icon::LocalProjectIconStore;
use persistence::storage::registry::{FileBackedProjectRegistry, FileBackedWorkspaceRegistry};
use serde_json::json;
use uuid::Uuid;

use crate::{Api, Services};

pub(crate) const TOKEN: &str = "in-process-test-token-at-least-32-characters";

/// An API whose directory holds two active workspaces in separate directories.
pub(crate) struct Workspaces {
    pub(crate) root: tempfile::TempDir,
    pub(crate) api: Api,
    pub(crate) shared: String,
    pub(crate) other: String,
    pub(crate) organization: Uuid,
}

impl Workspaces {
    /// Build the API for `address` with a shared and an unrelated workspace.
    pub(crate) fn new(address: SocketAddr) -> Self {
        let root = tempfile::tempdir().unwrap();
        for name in ["shared", "other", "registry"] {
            std::fs::create_dir(root.path().join(name)).unwrap();
        }
        let registry = root.path().join("registry");
        let directory = Directory::new(DirectoryDependencies {
            projects: Box::new(FileBackedProjectRegistry::new(
                registry.join("projects.json"),
            )),
            workspaces: Box::new(FileBackedWorkspaceRegistry::new(
                registry.join("workspaces.json"),
            )),
            source: Box::new(LocalDirectorySource),
            config_store: Box::new(LocalProjectConfigStore),
            icon_store: Box::new(LocalProjectIconStore::new(registry.join("icons"))),
            server_id: "test-server".into(),
        });
        let open = |name: &str| {
            let path = root.path().join(name);
            directory
                .open_workspace(path.to_str().unwrap(), "2026-10-30T00:00:00Z")
                .unwrap()
                .workspace_id
        };
        let (shared, other) = (open("shared"), open("other"));
        let mut api = Api::new(
            address,
            "test-server".into(),
            "instance".into(),
            TOKEN.into(),
            Services::default(),
        )
        .unwrap();
        let state = Arc::get_mut(&mut api.shared).unwrap();
        Arc::get_mut(&mut state.metadata).unwrap().directory =
            Some(crate::shared_service(directory));
        Self {
            root,
            api,
            shared,
            other,
            organization: Uuid::new_v4(),
        }
    }

    /// Canonical directory of the shared workspace.
    pub(crate) fn shared_path(&self) -> std::path::PathBuf {
        self.root.path().join("shared").canonicalize().unwrap()
    }

    /// Archive workspace `id` directly in the directory.
    pub(crate) fn archive(&self, id: &str) {
        self.api
            .shared
            .metadata
            .directory
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .archive_workspace(id, "2026-10-30T01:00:00Z")
            .unwrap();
    }

    /// Configure an organization-wide share of the shared workspace and accept it.
    pub(crate) async fn accept_share(&self) -> Visitor {
        let request = serde_json::from_value(json!({
            "workspaceKey": self.shared,
            "organizationId": self.organization,
            "permission": "operate",
            "audience": "organization",
        }))
        .unwrap();
        self.api.shared.shares.set(request).await.unwrap();
        let share_id = Uuid::new_v4();
        self.api.share_book().apply(Outcome {
            revision: None,
            accepted: vec![Accepted {
                share_id,
                workspace_key: self.shared.clone(),
                organization_id: self.organization,
                generation: 1,
                missing_recipients: Vec::new(),
            }],
            rejected: Vec::new(),
            orgs: Vec::new(),
        });
        Visitor {
            user_id: Uuid::new_v4(),
            display_name: Some("Guest".into()),
            organization_id: self.organization,
            share_id,
            workspace_key: self.shared.clone(),
            permission: "operate".into(),
        }
    }
}

/// An unassigned-port-free address for in-process APIs that never listen.
pub(crate) fn address() -> SocketAddr {
    "127.0.0.1:7316".parse().unwrap()
}
