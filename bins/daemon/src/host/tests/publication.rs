use host_link::{CredentialStore, ManagedRelay, State};
use serde_json::Value;

use super::*;

fn logged_out_state(data: &std::path::Path) -> State {
    let lease = InstanceLease::acquire(data).unwrap();
    State {
        authorization_id: None,
        disconnected: false,
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
async fn daemon_starts_before_login_and_rejects_credentials_from_another_installation() {
    let root = tempfile::tempdir().unwrap();
    let configuration = config(root.path());
    let server = Server::bind(configuration.clone()).await.unwrap();
    server.serve(async {}).await.unwrap();

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
    let configuration = config(root.path());
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
                && value["phase"] == "unauthorized"
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
    assert!(!relay.online().await);
    assert!(!relay.hub_active().await);
    assert!(!relay.hub_online().await);
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
    assert!(!relay.hub_online().await);
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

#[tokio::test]
async fn authenticated_handoff_and_disconnect_share_durable_authorization() {
    let root = tempfile::tempdir().unwrap();
    let server = Server::bind(config(root.path())).await.unwrap();
    let address = server.address();
    let (stop, signal) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(server.serve(async move {
        let _ = signal.await;
    }));
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let authorization_url = format!("http://{address}/api/device/authorization");
    let hub_url = format!("http://{address}/api/hub/control");
    let bearer = "offline-host-test-token-at-least-32-characters";
    let response = client.get(&authorization_url).send().await.unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
    let response = client.put(&authorization_url)
        .json(&serde_json::json!({"kind":"enrollment","token":"fixture-private-secret","request_id":uuid::Uuid::new_v4()}))
        .send().await.unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
    assert!(!root.path().join("device/.env").exists());

    assert_hub_controls(&client, &authorization_url, &hub_url, bearer).await;
    let id = uuid::Uuid::new_v4();
    let handoff = serde_json::json!({
        "kind":"device_code", "device_code":"c".repeat(64), "user_code":"ABCD-0123-ABCD",
        "request_id":id, "expires_at":Utc::now()+chrono::Duration::minutes(9), "interval":300,
    });
    assert_invalid_handoff(&client, &authorization_url, bearer).await;
    for _ in 0..2 {
        assert_eq!(
            client
                .put(&authorization_url)
                .bearer_auth(bearer)
                .json(&handoff)
                .send()
                .await
                .unwrap()
                .status(),
            reqwest::StatusCode::ACCEPTED
        );
    }
    let status: Value = client
        .get(&authorization_url)
        .bearer_auth(bearer)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status["management"]["phase"], "connecting");
    assert!(!status.to_string().contains(&"c".repeat(64)));
    assert!(!status.to_string().contains("ABCD-0123-ABCD"));
    let response = client.put(&hub_url).bearer_auth(bearer)
        .json(&serde_json::json!({"hub_ticket":"d".repeat(64),"node_session_id":uuid::Uuid::new_v4(),"center_url":"https://aitapp.cn/api"}))
        .send().await.unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::CONFLICT);
    for url in [&hub_url, &format!("http://{address}/api/relay/control")] {
        assert_eq!(
            client
                .delete(url)
                .bearer_auth(bearer)
                .send()
                .await
                .unwrap()
                .status(),
            reqwest::StatusCode::NO_CONTENT
        );
        // A fresh, explicit handoff is needed to publish again.
        if url == &hub_url {
            assert_eq!(
                client
                    .put(&authorization_url)
                    .bearer_auth(bearer)
                    .json(&handoff)
                    .send()
                    .await
                    .unwrap()
                    .status(),
                reqwest::StatusCode::ACCEPTED
            );
        }
    }

    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let store = crate::device::Store::open(root.path()).unwrap();
    let state = store.load().unwrap().unwrap();
    assert!(state.disconnected);
    assert!(
        state.credential.is_none() && state.pending.is_none() && state.authorization_id.is_none()
    );
    assert!(!crate::device::Store::credentials_present(root.path()).unwrap());
    drop(store);
    assert_disconnected_after_restart(root.path()).await;
}

async fn assert_hub_controls(
    client: &reqwest::Client,
    authorization_url: &str,
    hub_url: &str,
    bearer: &str,
) {
    let status: Value = client
        .get(authorization_url)
        .bearer_auth(bearer)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status["management"]["autonomous"], true);
    assert_eq!(status["management"]["phase"], "unauthorized");
    assert_eq!(
        client
            .delete(hub_url)
            .bearer_auth(bearer)
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::NO_CONTENT
    );
    assert_eq!(
        client
            .put(hub_url)
            .bearer_auth(bearer)
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::CONFLICT
    );
    let status: Value = client
        .get(authorization_url)
        .bearer_auth(bearer)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status["management"]["phase"], "disconnected");
    assert_eq!(status["management"]["binding"], Value::Null);
    assert_eq!(status["hub"]["status"]["online"], false);
    assert_eq!(status["relay"]["online"], false);
    assert_eq!(
        client
            .delete(authorization_url)
            .bearer_auth(bearer)
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::NO_CONTENT
    );
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn exchange(socket: &mut Socket, value: Value) -> Value {
    use futures_util::{SinkExt, StreamExt};
    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            value.to_string().into(),
        ))
        .await
        .unwrap();
    let message = tokio::time::timeout(Duration::from_secs(3), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    serde_json::from_str(message.to_text().unwrap()).unwrap()
}

#[tokio::test]
async fn publication_rpc_negotiates_and_uses_the_same_runtime_as_http() {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let root = tempfile::tempdir().unwrap();
    let server = Server::bind(config(root.path())).await.unwrap();
    let address = server.address();
    let (stop, signal) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(server.serve(async move {
        let _ = signal.await;
    }));
    let mut request = format!("ws://{address}/v1/ws")
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        "authorization",
        "Bearer offline-host-test-token-at-least-32-characters"
            .parse()
            .unwrap(),
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    let methods = [
        "publication.status.request",
        "publication.authorize.request",
        "publication.stop.request",
        "relay.stop.request",
    ];
    let hello = exchange(&mut socket, serde_json::json!({"type":"hello", "client_id":"publication-test", "protocol":{"major":1,"min_minor":0,"max_minor":0}, "capabilities":methods, "required_capabilities":["connection.single.v1"]})).await;
    assert_eq!(hello["type"], "server_info");
    for (method, phase) in [
        ("publication.status.request", "unauthorized"),
        ("publication.stop.request", "disconnected"),
    ] {
        let value = exchange(&mut socket, serde_json::json!({"type":"request","request_id":uuid::Uuid::new_v4(),"method":method,"params":{}})).await;
        assert_eq!(value["result"]["management"]["phase"], phase);
        assert_eq!(value["result"]["management"]["autonomous"], true);
    }
    let handoff = serde_json::json!({"kind":"device_code","device_code":"a".repeat(64),"user_code":"ABCD-0123-ABCD","request_id":uuid::Uuid::new_v4(),"expires_at":Utc::now()+chrono::Duration::minutes(9),"interval":300});
    let value = exchange(&mut socket, serde_json::json!({"type":"request","request_id":uuid::Uuid::new_v4(),"method":"publication.authorize.request","params":{"authorization":handoff,"mode":"initial"}})).await;
    assert_eq!(value["result"]["management"]["phase"], "disconnected");
    let value = exchange(&mut socket, serde_json::json!({"type":"request","request_id":uuid::Uuid::new_v4(),"method":"publication.authorize.request","params":{"authorization":handoff}})).await;
    assert_eq!(value["result"]["management"]["phase"], "connecting");
    assert!(!value.to_string().contains(&"a".repeat(64)));
    let value = exchange(&mut socket, serde_json::json!({"type":"request","request_id":uuid::Uuid::new_v4(),"method":"relay.stop.request","params":{}})).await;
    assert_eq!(value["result"]["management"]["phase"], "disconnected");
    assert_eq!(value["result"]["status"]["online"], false);
    socket.close(None).await.unwrap();
    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

async fn assert_disconnected_after_restart(data: &std::path::Path) {
    let server = Server::bind(config(data)).await.unwrap();
    let address = server.address();
    let (stop, signal) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(server.serve(async move {
        let _ = signal.await;
    }));
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let value: Value = client
        .get(format!("http://{address}/api/device/authorization"))
        .bearer_auth("offline-host-test-token-at-least-32-characters")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(value["management"]["phase"], "disconnected");
    assert_eq!(value["management"]["binding"], Value::Null);
    assert_eq!(value["hub"]["status"]["connecting"], false);
    assert_eq!(value["relay"]["connecting"], false);
    stop.send(()).unwrap();
    task.await.unwrap().unwrap();
}

async fn assert_invalid_handoff(client: &reqwest::Client, authorization_url: &str, bearer: &str) {
    // Bad input is rejected without echoing the submitted secret or disturbing the current state.
    let response = client.put(authorization_url).bearer_auth(bearer)
        .json(&serde_json::json!({"kind":"enrollment","token":"fixture-private-secret","request_id":"bad"}))
        .send().await.unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    assert!(
        !response
            .text()
            .await
            .unwrap()
            .contains("fixture-private-secret")
    );
}
