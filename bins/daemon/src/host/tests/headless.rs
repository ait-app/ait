use host_link::{CredentialStore, ManagedRelay, State};
use serde_json::Value;

use super::*;

fn logged_out_state(data: &std::path::Path) -> State {
    let lease = InstanceLease::acquire(data).unwrap();
    State {
        machine: host_link::Machine {
            server_id: lease.server_id,
            display_name: "fixture-host".into(),
            platform: "linux".into(),
            app_version: "fixture".into(),
        },
        credential: None,
        pending: None,
    }
}

#[tokio::test]
async fn headless_requires_login_and_rejects_another_installation_without_holding_the_lock() {
    let root = tempfile::tempdir().unwrap();
    let mut configuration = config(root.path());
    configuration.headless = true;
    let error = Server::bind(configuration.clone()).await.err().unwrap();
    assert!(error.to_string().contains("run daemon login"));

    let mut state = logged_out_state(root.path());
    state.machine.server_id = uuid::Uuid::new_v4();
    crate::device::Store::open(root.path())
        .unwrap()
        .save(&state)
        .unwrap();
    let error = Server::bind(configuration).await.err().unwrap();
    assert!(error.to_string().contains("another installation"));
    assert!(InstanceLease::acquire(root.path()).is_ok());
    assert!(crate::device::Store::open(root.path()).is_ok());
}

#[tokio::test]
async fn publication_failure_preserves_local_http_and_shutdown_releases_device_storage() {
    let root = tempfile::tempdir().unwrap();
    let state = logged_out_state(root.path());
    crate::device::Store::open(root.path())
        .unwrap()
        .save(&state)
        .unwrap();
    let mut configuration = config(root.path());
    configuration.headless = true;
    let server = Server::bind(configuration).await.unwrap();
    let address = server.address();
    let (stop, signal) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(server.serve(async move {
        let _ = signal.await;
    }));
    let snapshot = root.path().join("device/status.json");
    let status = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(bytes) = std::fs::read(&snapshot)
                && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
                && value["phase"] == "reauthorization_required"
            {
                break value;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(status["binding"], Value::Null);
    assert_eq!(status["pid"], std::process::id());
    assert!(status["instance_id"].as_str().is_some());
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(format!("http://{address}/v1/server/info"))
        .bearer_auth("offline-host-test-token-at-least-32-characters")
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());

    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    assert!(InstanceLease::acquire(root.path()).is_ok());
    assert!(crate::device::Store::open(root.path()).is_ok());
}

#[tokio::test]
async fn status_relay_rejects_invalid_tickets_and_cleans_failed_snapshot_publication() {
    let root = tempfile::tempdir().unwrap();
    let mut api = Api::new(
        "127.0.0.1:7316".parse().unwrap(),
        uuid::Uuid::new_v4().to_string(),
        uuid::Uuid::new_v4().to_string(),
        config(root.path()).token,
        Services::default(),
    )
    .unwrap();
    let relay = crate::device::StatusRelay::new(
        api.claim_managed_relay().unwrap(),
        root.path(),
        uuid::Uuid::new_v4(),
    );
    assert!(!relay.active().await);
    assert!(!relay.hub_active().await);
    assert_eq!(
        relay
            .start(uuid::Uuid::new_v4(), "invalid-fixture-ticket".into())
            .await
            .unwrap_err(),
        host_link::Error::Unavailable
    );
    assert_eq!(
        relay
            .start_hub(uuid::Uuid::new_v4(), "invalid-fixture-ticket".into())
            .await
            .unwrap_err(),
        host_link::Error::Unavailable
    );
    // This current-thread test stops the connector before its spawned network task is polled.
    relay
        .start_hub(uuid::Uuid::new_v4(), "a".repeat(64).into())
        .await
        .unwrap();
    assert!(relay.hub_active().await);
    relay.stop().await;
    assert!(!relay.hub_active().await);
    std::fs::create_dir_all(root.path().join("device/status.json")).unwrap();

    relay.report("running", None);

    let entries: Vec<_> = std::fs::read_dir(root.path().join("device"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(entries, [std::ffi::OsString::from("status.json")]);
    api.begin_shutdown();
    api.wait_closed().await;
}
