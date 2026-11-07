use std::os::unix::fs::PermissionsExt;

use super::*;

async fn fixture(ignore_term: bool) -> (tempfile::TempDir, Transport, u32) {
    let directory = tempfile::tempdir().unwrap();
    let program = directory.path().join("pi");
    std::fs::write(&program, include_str!("shutdown.py")).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    if ignore_term {
        std::fs::write(directory.path().join("ignore-term"), "").unwrap();
    }
    let client = PiClient::new(program);
    let mut transport =
        Transport::spawn(&client, directory.path().to_str().unwrap(), &[], None).unwrap();
    let ready = tokio::time::timeout(Duration::from_secs(5), transport.receive())
        .await
        .unwrap()
        .unwrap();
    let pid = u32::try_from(ready["pid"].as_u64().unwrap()).unwrap();
    (directory, transport, pid)
}

fn assert_exited(pid: u32) {
    assert!(
        !std::process::Command::new("/bin/kill")
            .args(["-0", "--", &pid.to_string()])
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success(),
        "process {pid} survived shutdown"
    );
}

#[tokio::test]
async fn close_allows_native_cleanup_of_detached_tools() {
    let (directory, mut transport, tool) = fixture(false).await;

    transport.close().await.unwrap();
    transport.close().await.unwrap();

    assert!(directory.path().join("cleaned").exists());
    assert_exited(tool);
}

#[test]
fn drop_cleans_up_detached_tools_after_runtime_shutdown() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (directory, transport, tool) = runtime.block_on(fixture(false));

    drop(runtime);
    drop(transport);

    let deadline = Instant::now() + Duration::from_secs(5);
    while !directory.path().join("cleaned").exists() {
        assert!(Instant::now() < deadline, "native cleanup did not finish");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_exited(tool);
}

#[tokio::test]
async fn cancelled_close_still_finishes_native_cleanup() {
    let (directory, mut transport, tool) = fixture(false).await;

    assert!(
        tokio::time::timeout(Duration::from_millis(20), transport.close())
            .await
            .is_err()
    );
    drop(transport);

    tokio::time::timeout(Duration::from_secs(5), async {
        while !directory.path().join("cleaned").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_exited(tool);
}

#[tokio::test]
async fn close_forces_and_reaps_unresponsive_process() {
    let (_directory, mut transport, process) = fixture(true).await;

    tokio::time::timeout(Duration::from_secs(5), transport.close())
        .await
        .unwrap()
        .unwrap();

    assert_exited(process);
}

#[tokio::test]
async fn timed_out_request_drops_transport_and_cleans_up_tools() {
    let (directory, transport, tool) = fixture(false).await;
    let request = async move {
        let mut transport = transport;
        transport.request(json!({"type":"get_commands"})).await
    };
    assert!(
        tokio::time::timeout(Duration::from_millis(20), request)
            .await
            .is_err()
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        while !directory.path().join("cleaned").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_exited(tool);
}

#[tokio::test]
async fn probe_keeps_success_when_cleanup_exceeds_budget() {
    let directory = tempfile::tempdir().unwrap();
    let program = directory.path().join("pi");
    std::fs::write(&program, include_str!("shutdown.py")).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    let client = PiClient::new(program);
    let result = crate::local::pi::probe::run(
        &client,
        directory.path().to_str().unwrap(),
        None,
        Some(Duration::from_secs(2)),
        |transport| {
            Box::pin(async move {
                let ready = transport.receive().await?;
                // Advance only after the real subprocess has installed its signal handler.
                tokio::time::pause();
                tokio::time::advance(Duration::from_secs(2)).await;
                Ok(ready)
            })
        },
    )
    .await;
    tokio::time::resume();
    let tool = u32::try_from(result.unwrap()["pid"].as_u64().unwrap()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !directory.path().join("cleaned").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_exited(tool);
}
