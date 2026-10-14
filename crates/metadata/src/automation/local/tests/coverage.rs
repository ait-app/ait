use super::*;

#[test]
fn setup_output_retains_both_ends_and_marks_the_omitted_middle() {
    let mut output = vec![b'x'; OUTPUT_BYTES + 100];
    output[..5].copy_from_slice(b"start");
    let tail = output.len() - 3;
    output[tail..].copy_from_slice(b"end");

    let (text, truncated) = bound_output(&output);

    assert!(truncated);
    assert!(text.len() <= 64 * 1024);
    assert!(text.starts_with("start"));
    assert!(text.ends_with("end"));
    assert!(text.contains("\n... setup output truncated ...\n"));
    assert_eq!(bound_output(b"short log"), ("short log".into(), false));
}

#[cfg(unix)]
#[test]
fn running_setup_output_overflow_terminates_the_owned_process_group() {
    let root = tempfile::tempdir().unwrap();
    let runtime = automation();
    let workspace = placement(root.path(), "output-overflow");

    let result = run_setup_command(
        &Arc::downgrade(&runtime.inner),
        &workspace,
        "head -c 8388609 /dev/zero; sleep 30",
        3000,
    );

    assert!(matches!(result, Err(WorkspaceAutomationError::Io(message))
        if message == "Setup command output exceeded 8 MiB"));
    assert!(lock(&runtime.inner.state).setup_cleanup.is_empty());
}

#[cfg(unix)]
#[test]
fn setup_cannot_restart_until_retained_child_cleanup_is_complete() {
    let root = tempfile::tempdir().unwrap();
    let runtime = automation();
    let workspace = placement(root.path(), "cleanup");
    let mut command = shell_command("sleep 30");
    configure_process_group(&mut command);
    lock(&runtime.inner.state)
        .setup_cleanup
        .insert(workspace.workspace_id.clone(), command.spawn().unwrap());

    let error = runtime.start_setup(&workspace).unwrap_err();
    assert!(matches!(error, WorkspaceAutomationError::Io(message)
        if message == "Previous setup cleanup is still pending"));
    runtime
        .close_workspaces(std::slice::from_ref(&workspace.workspace_id))
        .unwrap();

    assert!(lock(&runtime.inner.state).setup_cleanup.is_empty());
    assert!(runtime.start_setup(&workspace).unwrap());
    assert_eq!(
        wait_for(&runtime, &workspace.workspace_id, SetupLifecycle::Completed).lifecycle,
        SetupLifecycle::Completed
    );
}

#[test]
fn missing_script_directory_fails_without_retaining_a_process() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("ait.json"),
        r#"{"scripts":{"fixture":{"command":"echo fixture"}}}"#,
    )
    .unwrap();
    let runtime =
        LocalWorkspaceAutomation::new(Arc::new(FixedConfigPath(root.path().join("ait.json"))));
    let mut workspace = placement(root.path(), "missing-cwd");
    workspace.cwd = root.path().join("absent").to_string_lossy().into_owned();

    assert!(matches!(
        runtime.start_script(&workspace, "fixture"),
        Err(WorkspaceAutomationError::Io(_))
    ));
    assert!(lock(&runtime.inner.state).scripts.is_empty());
    assert!(format!("{runtime:?}").contains("Inner"));
}

#[derive(Debug)]
struct FixedConfigPath(std::path::PathBuf);

impl ProjectConfigStore for FixedConfigPath {
    fn config_path(&self, _: &Path) -> std::io::Result<std::path::PathBuf> {
        Ok(self.0.clone())
    }
    fn read(
        &self,
        _: &str,
    ) -> Result<
        domain::storage::project::ProjectConfigDocument,
        domain::storage::project::ProjectConfigStoreError,
    > {
        Err(domain::storage::project::ProjectConfigStoreError::Invalid)
    }
    fn write(
        &self,
        _: &str,
        _: &Value,
        _: Option<domain::storage::project::ProjectConfigRevision>,
    ) -> Result<
        domain::storage::project::ProjectConfigWrite,
        domain::storage::project::ProjectConfigStoreError,
    > {
        Err(domain::storage::project::ProjectConfigStoreError::Write)
    }
}
