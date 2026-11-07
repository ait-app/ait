use super::*;

#[cfg(target_os = "linux")]
#[tokio::test(start_paused = true)]
async fn delayed_runtime_poll_grants_the_force_budget_after_signalling() {
    let directory = tempfile::tempdir().unwrap();
    let mut command = tokio::process::Command::new("python");
    command.args(["-c", "import time\nprint('{\"jsonrpc\":\"2.0\",\"method\":\"ready\"}',flush=True)\ntime.sleep(120)"]);
    let mut transport = Transport::spawn(
        command,
        directory.path().to_str().unwrap(),
        Duration::from_secs(2),
        "acp-test",
    )
    .unwrap();
    transport.receive().await.unwrap();
    let process = transport.child.id().unwrap();
    let mut close = Box::pin(transport.close());
    assert!(futures_util::poll!(close.as_mut()).is_pending());
    tokio::time::advance(Duration::from_secs(3)).await;
    assert!(futures_util::poll!(close.as_mut()).is_pending());
    tokio::task::spawn_blocking(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            let status = std::fs::read_to_string(format!("/proc/{process}/stat")).unwrap();
            if status.rsplit_once(") ").unwrap().1.starts_with('Z') {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "fixture did not finish"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    })
    .await
    .unwrap();
    close.await.unwrap();
    assert!(transport.child.try_wait().unwrap().is_some());
}

#[tokio::test]
async fn close_sends_eof_flushes_the_peer_and_is_idempotent() {
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("flushed");
    let mut command = tokio::process::Command::new("python");
    command
        .args([
            "-c",
            "import sys,pathlib; print('{\"jsonrpc\":\"2.0\",\"method\":\"ready\"}',flush=True); sys.stdin.read(); pathlib.Path(sys.argv[1]).write_text('flushed')",
        ])
        .arg(&marker);
    let mut transport = Transport::spawn(
        command,
        directory.path().to_str().unwrap(),
        Duration::from_secs(2),
        "acp-test",
    )
    .unwrap();
    assert_eq!(transport.receive().await.unwrap()["method"], "ready");
    transport.close().await.unwrap();
    assert_eq!(std::fs::read_to_string(marker).unwrap(), "flushed");
    assert!(transport.child.try_wait().unwrap().is_some());
    transport.close().await.unwrap();
}

#[tokio::test]
async fn close_forces_an_uncooperative_peer_and_reaps_it() {
    let directory = tempfile::tempdir().unwrap();
    let mut command = tokio::process::Command::new("python");
    command.args(["-c", "import time; time.sleep(120)"]);
    let mut transport = Transport::spawn(
        command,
        directory.path().to_str().unwrap(),
        Duration::from_secs(2),
        "acp-test",
    )
    .unwrap();
    tokio::time::timeout(Duration::from_secs(3), transport.close())
        .await
        .unwrap()
        .unwrap();
    assert!(transport.child.try_wait().unwrap().is_some());
}
