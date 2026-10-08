use std::sync::Arc;

use serde_json::{Value, json};

use super::*;
use crate::rpc::directory::execute;

#[test]
fn registry_outages_fail_reads_and_mutations_without_claiming_success() {
    let mut directory = directory();
    directory.projects = Arc::new(Projects {
        records: Mutex::new(vec![project()]),
        unavailable: true,
    });
    for (method, params) in [
        ("project.list.request", json!({})),
        ("workspace.list.request", json!({})),
        (
            "project.rename.request",
            json!({"projectId":"prj_alpha","customName":"renamed"}),
        ),
        (
            "project.config.read.request",
            json!({"repoRoot":"/tmp/alpha"}),
        ),
        (
            "project.config.write.request",
            json!({"repoRoot":"/tmp/alpha","config":{},"expectedRevision":null}),
        ),
        ("project.icon.get.request", json!({"projectId":"prj_alpha"})),
        (
            "project.icon.set.request",
            json!({"projectId":"prj_alpha","source":{"type":"automatic"}}),
        ),
        ("project.add.request", json!({"cwd":"/tmp/alpha"})),
        ("workspace.open.request", json!({"cwd":"/tmp/alpha"})),
    ] {
        assert_eq!(
            execute(&mut directory, method, params),
            Err(crate::rpc::ErrorCode::RegistryIo),
            "{method}"
        );
    }
    assert_eq!(
        execute(&mut directory, "unknown", json!({})),
        Err(crate::rpc::ErrorCode::MethodNotFound)
    );
    assert_eq!(directory.workspaces.list().unwrap().len(), 1);
}

#[derive(Debug)]
struct BrokenConfig(ProjectConfigStoreError);

impl ProjectConfigStore for BrokenConfig {
    fn config_path(&self, _: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
        Err(std::io::Error::other("broken config"))
    }

    fn read(&self, _: &str) -> Result<ProjectConfigDocument, ProjectConfigStoreError> {
        Err(self.0)
    }

    fn write(
        &self,
        _: &str,
        _: &Value,
        _: Option<StoreConfigRevision>,
    ) -> Result<ProjectConfigWrite, ProjectConfigStoreError> {
        Err(self.0)
    }
}

#[test]
fn configuration_failures_are_inline_and_do_not_replace_saved_content() {
    let mut directory = directory();
    for root in ["/missing", "/unregistered"] {
        for (method, params) in [
            ("project.config.read.request", json!({"repoRoot":root})),
            (
                "project.config.write.request",
                json!({"repoRoot":root,"config":{},"expectedRevision":null}),
            ),
        ] {
            let result = execute(&mut directory, method, params).unwrap();
            assert_eq!(result["ok"], false);
            assert_eq!(result["error"]["code"], "project_not_found");
            assert_eq!(result["repoRoot"], root);
        }
    }
    let store = Arc::new(ConfigStore::default());
    directory.config_store = store.clone();
    let written = execute(
        &mut directory,
        "project.config.write.request",
        json!({"repoRoot":"/tmp/alpha","config":{"future":true},"expectedRevision":null}),
    )
    .unwrap();
    let stale = execute(
        &mut directory,
        "project.config.write.request",
        json!({"repoRoot":"/tmp/alpha","config":{},"expectedRevision":null}),
    )
    .unwrap();
    assert_eq!(stale["error"]["code"], "stale_project_config");
    assert_eq!(stale["error"]["currentRevision"], written["revision"]);
    assert_eq!(
        store.read("/tmp/alpha").unwrap().config,
        Some(json!({"future":true}))
    );
    let updated = execute(&mut directory, "project.config.write.request", json!({"repoRoot":"/tmp/alpha","config":{"future":"updated"},"expectedRevision":written["revision"]})).unwrap();
    assert_eq!(updated["ok"], true);
    assert_eq!(
        store.read("/tmp/alpha").unwrap().config,
        Some(json!({"future":"updated"}))
    );
    store.0.lock().unwrap().as_mut().unwrap().0 = json!({"worktree":false});
    let result = execute(
        &mut directory,
        "project.config.read.request",
        json!({"repoRoot":"/tmp/alpha"}),
    )
    .unwrap();
    assert_eq!(result["error"]["code"], "invalid_project_config");
    for error in [
        ProjectConfigStoreError::Invalid,
        ProjectConfigStoreError::Write,
    ] {
        directory.config_store = Arc::new(BrokenConfig(error));
        let result = execute(
            &mut directory,
            "project.config.read.request",
            json!({"repoRoot":"/tmp/alpha"}),
        )
        .unwrap();
        assert_eq!(result["error"]["code"], "invalid_project_config");
        let result = execute(
            &mut directory,
            "project.config.write.request",
            json!({"repoRoot":"/tmp/alpha","config":{},"expectedRevision":null}),
        )
        .unwrap();
        assert_eq!(result["error"]["code"], "write_failed");
    }
}

#[derive(Debug)]
struct BrokenIcons(ProjectIconStoreError);

impl ProjectIconStore for BrokenIcons {
    fn write_custom(&self, _: &str, _: &[u8]) -> Result<(), ProjectIconStoreError> {
        Err(self.0)
    }
    fn remove_custom(&self, _: &str) -> Result<(), ProjectIconStoreError> {
        Err(self.0)
    }
    fn read_custom(&self, _: &str) -> Result<Option<ProjectIcon>, ProjectIconStoreError> {
        Err(self.0)
    }
    fn find_automatic(&self, _: &str) -> Result<Option<ProjectIcon>, ProjectIconStoreError> {
        Err(self.0)
    }
}

#[test]
fn icon_failures_keep_the_previous_revision_and_never_claim_acceptance() {
    let mut directory = directory();
    for (project, source, expected) in [
        (
            "prj_a",
            json!({"type":"upload","data":"!"}),
            "Unsupported or invalid icon file",
        ),
        ("missing", json!({"type":"automatic"}), "Project not found"),
    ] {
        let result = execute(
            &mut directory,
            "project.icon.set.request",
            json!({"projectId":project,"source":source}),
        )
        .unwrap();
        assert_eq!(result["accepted"], false);
        assert_eq!(result["error"], expected);
    }
    assert_eq!(
        execute(
            &mut directory,
            "project.icon.get.request",
            json!({"projectId":"missing"})
        )
        .unwrap()["error"],
        "Project not found"
    );
    for error in [ProjectIconStoreError::Invalid, ProjectIconStoreError::Io] {
        directory.icon_store = Arc::new(BrokenIcons(error));
        for source in [
            json!({"type":"automatic"}),
            json!({"type":"upload","data":"aWNvbg=="}),
        ] {
            let result = execute(
                &mut directory,
                "project.icon.set.request",
                json!({"projectId":"prj_a","source":source}),
            )
            .unwrap();
            assert_eq!(result["accepted"], false);
            assert!(result["error"].is_string());
            assert!(
                directory
                    .projects
                    .get("prj_a")
                    .unwrap()
                    .unwrap()
                    .custom_icon_revision
                    .is_none()
            );
        }
        let result = execute(
            &mut directory,
            "project.icon.get.request",
            json!({"projectId":"prj_a"}),
        )
        .unwrap();
        assert!(result["icon"].is_null());
        assert!(result["error"].is_string());
    }
}

#[derive(Debug)]
struct FailedSource {
    operation: &'static str,
    error: DirectorySourceError,
    rollback: Mutex<Vec<String>>,
}

#[test]
fn unreadable_directories_return_business_errors_without_registering_partial_records() {
    let mut directory = directory();
    directory.source = Arc::new(FailedSource {
        operation: "inspect",
        error: DirectorySourceError::PermissionDenied,
        rollback: Mutex::new(Vec::new()),
    });
    for method in ["project.add.request", "workspace.open.request"] {
        let result = execute(&mut directory, method, json!({"cwd":"/unreadable"})).unwrap();
        assert!(result["error"].is_string());
        assert!(result["project"].is_null() && result["workspace"].is_null());
    }
    assert_eq!(directory.projects.list().unwrap().len(), 1);
    assert_eq!(directory.workspaces.list().unwrap().len(), 1);
}

impl DirectorySource for FailedSource {
    fn inspect(&self, path: &str) -> Result<Checkout, DirectorySourceError> {
        if self.operation == "inspect" || (self.operation == "register" && path.ends_with("/child"))
        {
            Err(self.error)
        } else {
            Source.inspect(path)
        }
    }
    fn create_child(&self, parent: &str, name: &str) -> Result<String, DirectorySourceError> {
        if self.operation == "create" {
            Err(self.error)
        } else {
            Source.create_child(parent, name)
        }
    }
    fn remove_empty(&self, path: &str) -> Result<(), DirectorySourceError> {
        self.rollback.lock().unwrap().push(path.to_owned());
        Err(self.error)
    }
    fn equivalent(&self, left: &str, right: &str) -> bool {
        left == right
    }
    fn canonical(&self, path: &str) -> Result<String, DirectorySourceError> {
        self.inspect(path).map(|checkout| checkout.cwd)
    }
}

#[test]
fn directory_creation_reports_the_failed_stage_and_retains_unremoved_paths() {
    let mut directory = directory();
    for operation in ["inspect", "create", "register"] {
        for (error, expected) in [
            (DirectorySourceError::NotFound, "parent_directory_not_found"),
            (
                DirectorySourceError::AlreadyExists,
                if operation == "inspect" {
                    "parent_directory_not_found"
                } else {
                    "directory_exists"
                },
            ),
            (DirectorySourceError::PermissionDenied, "permission_denied"),
            (DirectorySourceError::Io, "filesystem_error"),
        ] {
            let source = Arc::new(FailedSource {
                operation,
                error,
                rollback: Mutex::new(Vec::new()),
            });
            directory.source = source.clone();
            let result = execute(
                &mut directory,
                "project.create_directory.request",
                json!({"parentPath":"/parent","name":"child"}),
            )
            .unwrap();
            assert!(result["project"].is_null(), "{result}");
            if operation == "register" {
                assert_eq!(result["errorCode"], "registration_failed");
                assert_eq!(result["directoryPath"], "/parent/child");
                assert_eq!(*source.rollback.lock().unwrap(), ["/parent/child"]);
            } else {
                assert_eq!(result["errorCode"], expected, "{result}");
                assert!(result["directoryPath"].is_null());
                assert!(source.rollback.lock().unwrap().is_empty());
            }
        }
    }
    assert_eq!(directory.projects.list().unwrap().len(), 1);
}

#[test]
fn missing_mutation_targets_remain_inline_and_invalid_creation_has_no_effects() {
    let mut directory = directory();
    for (method, params) in [
        (
            "project.rename.request",
            json!({"projectId":"missing","customName":"name"}),
        ),
        (
            "workspace.title.set.request",
            json!({"workspaceId":"missing","title":"title"}),
        ),
        (
            "workspace.pin.set.request",
            json!({"workspaceId":"missing","pinned":true}),
        ),
    ] {
        let result = execute(&mut directory, method, params).unwrap();
        assert_eq!(result["accepted"], false);
        assert!(result["error"].is_string());
    }
    let result = execute(
        &mut directory,
        "workspace.archive.request",
        json!({"workspaceId":"missing"}),
    )
    .unwrap();
    assert!(result["archivedAt"].is_null());
    for method in ["workspace.open.request", "project.add.request"] {
        let result = execute(&mut directory, method, json!({"cwd":"/missing"})).unwrap();
        assert_eq!(result["errorCode"], "directory_not_found");
    }
    for name in ["", "..", "a/b", " name "] {
        let result = execute(
            &mut directory,
            "project.create_directory.request",
            json!({"parentPath":"/parent","name":name}),
        )
        .unwrap();
        assert_eq!(result["errorCode"], "invalid_name");
    }
    assert_eq!(directory.workspaces.list().unwrap().len(), 1);
    assert_eq!(directory.projects.list().unwrap().len(), 1);
}
