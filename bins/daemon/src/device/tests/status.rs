use super::*;
use host_link::ManagedRelay;

#[tokio::test]
async fn cancelled_status_writes_keep_the_lease_and_cannot_overwrite_newer_reports() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("device")).unwrap();
    let instance = Arc::new(InstanceLease::acquire(root.path()).unwrap());
    let mut api = api::Api::new(
        "127.0.0.1:7316".parse().unwrap(),
        instance.server_id.to_string(),
        instance.instance_id.to_string(),
        "status-test-local-token-at-least-32-characters".into(),
        api::Services::default(),
    )
    .unwrap();
    let relay = StatusRelay::new(api.claim_managed_relay().unwrap(), root.path(), instance);
    let writer = relay.writer.clone();
    let (locked, ready) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let blocking_writer = std::thread::spawn(move || {
        let _guard = writer.lock().unwrap();
        locked.send(()).unwrap();
        let _ = released.recv();
    });
    ready.recv().unwrap();
    let earlier = relay.clone();
    let pending = tokio::spawn(async move { earlier.report("connecting", None).await });
    while relay.revision.load(Ordering::Relaxed) < 1 {
        tokio::task::yield_now().await;
    }
    pending.abort();
    assert!(pending.await.unwrap_err().is_cancelled());
    let later = relay.clone();
    let finished = tokio::spawn(async move { later.report("disconnected", None).await });
    while relay.revision.load(Ordering::Relaxed) < 2 {
        tokio::task::yield_now().await;
    }
    drop(relay);
    assert!(InstanceLease::held(root.path()));
    release.send(()).unwrap();
    blocking_writer.join().unwrap();
    finished.await.unwrap();
    let status: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.path().join("device/status.json")).unwrap())
            .unwrap();
    assert_eq!(status["phase"], "disconnected");
    api.begin_shutdown();
    api.wait_closed().await;
}
