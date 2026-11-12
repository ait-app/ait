use super::*;

async fn confined_fixture(ignore_term: bool) -> (tempfile::TempDir, Transport, u32) {
    let directory = tempfile::tempdir().unwrap();
    let program = directory.path().join("pi");
    std::fs::write(&program, include_str!("../shutdown.py")).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::create_dir_all(directory.path().join(".git")).unwrap();
    if ignore_term {
        std::fs::write(directory.path().join("ignore-term"), "").unwrap();
    }
    let mut client = PiClient::new(program);
    client.environment =
        serde_json::from_value(json!({"HOME":directory.path().join("home")})).unwrap();
    let record = serde_json::from_value(json!({
        "workspaceId":"wks_0123456789abcdef", "projectId":"prj_1", "kind":"worktree",
        "cwd":directory.path(), "worktreeRoot":directory.path(), "displayName":"sandbox",
        "createdAt":"2026-01-01T00:00:00Z", "updatedAt":"2026-01-01T00:00:00Z",
        "sandbox":"worktree_writes", "archivedAt":null
    }))
    .unwrap();
    let confinement = domain::workspace::sandbox::WriteConfinement::for_workspace(&record);
    let mut transport = Transport::spawn(
        &client,
        directory.path().to_str().unwrap(),
        &[],
        confinement.as_ref(),
    )
    .unwrap();
    tokio::time::timeout(Duration::from_secs(5), transport.receive())
        .await
        .unwrap()
        .unwrap();
    let monitor = transport.group.unwrap();
    (directory, transport, monitor)
}

#[tokio::test]
async fn sandboxed_close_waits_for_native_cleanup_before_stopping_the_monitor() {
    let (directory, mut transport, monitor) = confined_fixture(false).await;
    transport.close().await.unwrap();
    assert_eq!(
        std::fs::read_to_string(directory.path().join("cleaned")).unwrap(),
        "done"
    );
    assert_exited(monitor);
}

#[tokio::test]
async fn sandboxed_close_still_forces_unresponsive_native_processes() {
    let (_directory, mut transport, monitor) = confined_fixture(true).await;
    tokio::time::timeout(Duration::from_secs(5), transport.close())
        .await
        .unwrap()
        .unwrap();
    assert_exited(monitor);
}

#[tokio::test]
async fn sandboxed_cancelled_close_finishes_cleanup_on_repeated_close() {
    let (directory, mut transport, monitor) = confined_fixture(false).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(20), transport.close())
            .await
            .is_err()
    );
    transport.close().await.unwrap();
    assert!(directory.path().join("cleaned").exists());
    assert_exited(monitor);
}

#[test]
fn sandboxed_drop_after_runtime_shutdown_still_cleans_native_tools() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (directory, transport, monitor) = runtime.block_on(confined_fixture(false));
    drop(runtime);
    drop(transport);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !directory.path().join("cleaned").exists()
        || std::path::Path::new(&format!("/proc/{monitor}")).exists()
    {
        assert!(
            Instant::now() < deadline,
            "sandboxed native cleanup did not finish"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
