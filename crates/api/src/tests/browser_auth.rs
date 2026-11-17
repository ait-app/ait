use std::io::{Seek, Write};

use filesystem::files::local::LocalFiles;
use filesystem::files::service::files::Files;
use model::download::GeneratedArtifact;

use super::*;

const ORIGIN: &str = "http://localhost:8081";

#[tokio::test]
async fn browser_ticket_requires_explicit_origin_and_bearer() {
    let fixture = Fixture::with_origins(Services::default(), vec![ORIGIN.to_owned()]).await;
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let endpoint = fixture.url(crate::browser_auth::TICKET_PATH);
    let preflight = client
        .request(Method::OPTIONS, &endpoint)
        .header("origin", ORIGIN)
        .header("access-control-request-method", "POST")
        .header("access-control-request-headers", "authorization")
        .send()
        .await
        .unwrap();
    assert_eq!(preflight.status(), StatusCode::NO_CONTENT);
    assert_eq!(preflight.headers()["access-control-allow-origin"], ORIGIN);
    assert!(
        !preflight
            .headers()
            .contains_key("access-control-allow-credentials")
    );
    for (origin, token, expected) in [
        (ORIGIN, "incorrect", StatusCode::UNAUTHORIZED),
        ("http://localhost:8082", TOKEN, StatusCode::FORBIDDEN),
        ("http://evil.test", TOKEN, StatusCode::FORBIDDEN),
    ] {
        let response = client
            .post(&endpoint)
            .header("origin", origin)
            .bearer_auth(token)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        if expected == StatusCode::UNAUTHORIZED {
            assert_eq!(response.headers()["access-control-allow-origin"], ORIGIN);
        }
    }
    assert_eq!(
        client
            .post(&endpoint)
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        client
            .post(format!("{endpoint}?token=forbidden"))
            .header("origin", ORIGIN)
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    fixture.stop().await;
}

#[tokio::test]
async fn browser_ticket_authenticates_real_upgrade_without_exposing_bearer() {
    let fixture = Fixture::with_origins(Services::default(), vec![ORIGIN.to_owned()]).await;
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let endpoint = fixture.url(crate::browser_auth::TICKET_PATH);
    let response = client
        .post(&endpoint)
        .header("origin", ORIGIN)
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    let value: Value = response.json().await.unwrap();
    let ticket = value["ticket"].as_str().unwrap();
    assert!(!value.to_string().contains(TOKEN));
    let protocol = format!("ait.ticket.{ticket}");
    let mut request = format!("ws://{}/v1/ws", fixture.address)
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("origin", ORIGIN.parse().unwrap());
    request
        .headers_mut()
        .insert("sec-websocket-protocol", protocol.parse().unwrap());
    let (mut socket, response) = connect_async(request.clone()).await.unwrap();
    assert_eq!(response.headers()["sec-websocket-protocol"], protocol);
    send(&mut socket, hello()).await;
    assert_eq!(receive(&mut socket).await["type"], "server_info");
    assert!(connect_async(request).await.is_err());
    // Origin permission does not remove Bearer authentication from other API routes.
    assert_eq!(
        client
            .get(fixture.url("/v1/server/info"))
            .header("origin", ORIGIN)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let mut invalid_host = HeaderMap::new();
    invalid_host.insert("host", "evil.test".parse().unwrap());
    invalid_host.insert("origin", ORIGIN.parse().unwrap());
    assert!(
        auth::validate_source(
            &invalid_host,
            &fixture.api.shared.authorities,
            &fixture.api.shared.browser_auth
        )
        .is_err()
    );
    socket.close(None).await.unwrap();
    fixture.stop().await;
}

#[tokio::test]
async fn browser_origins_must_be_configured_before_sharing_and_shutdown_rejects_tickets() {
    let fixture = Fixture::with_origins(Services::default(), vec![ORIGIN.to_owned()]).await;
    assert!(matches!(
        fixture.api.clone().with_browser_origins(Vec::new()),
        Err(ConfigError::SharedConfiguration)
    ));
    fixture.api.begin_shutdown();
    let response = browser_ticket(State(fixture.api.shared.clone()), HeaderMap::new()).await;
    assert_eq!(response.err().unwrap().0, StatusCode::SERVICE_UNAVAILABLE);
    fixture.stop().await;
}

#[tokio::test]
async fn browser_artifact_download_preserves_origin_policy_and_single_use_grants() {
    let root = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut api = Api::new(
        listener.local_addr().unwrap(),
        "download".to_owned(),
        "instance".to_owned(),
        TOKEN.into(),
        Services::default(),
    )
    .unwrap()
    .with_browser_origins(vec![ORIGIN.to_owned()])
    .unwrap();
    let mut files = Files::new(Box::new(LocalFiles::new(
        root.path().to_owned(),
        root.path(),
    )));
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(b"# saved\n").unwrap();
    file.rewind().unwrap();
    files
        .issue_artifact(
            "browser-export".to_owned(),
            GeneratedArtifact {
                agent_id: "agent".to_owned(),
                file_name: "timeline.md".to_owned(),
                mime_type: "text/markdown".to_owned(),
                size: 8,
                file,
            },
        )
        .unwrap();
    let state = Arc::get_mut(&mut api.shared).unwrap();
    Arc::get_mut(&mut state.filesystem).unwrap().files = Some(shared_service(files));
    let fixture = Fixture::serve(listener, api);
    let endpoint = fixture.url("/api/files/download?token=browser-export");
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    for method in [Method::GET, Method::OPTIONS] {
        let denied = client
            .request(method, &endpoint)
            .header("origin", "http://evil.test")
            .send()
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        assert!(!denied.headers().contains_key("access-control-allow-origin"));
    }
    let preflight = client
        .request(Method::OPTIONS, &endpoint)
        .header("origin", ORIGIN)
        .header("access-control-request-method", "GET")
        .header("access-control-request-headers", "authorization")
        .send()
        .await
        .unwrap();
    assert_eq!(preflight.status(), StatusCode::NO_CONTENT);
    assert_eq!(preflight.headers()["access-control-allow-origin"], ORIGIN);
    assert!(
        !preflight
            .headers()
            .contains_key("access-control-allow-credentials")
    );
    let downloaded = client
        .get(&endpoint)
        .header("origin", ORIGIN)
        .send()
        .await
        .unwrap();
    assert_eq!(downloaded.status(), StatusCode::OK);
    assert_eq!(downloaded.headers()["access-control-allow-origin"], ORIGIN);
    assert_eq!(downloaded.bytes().await.unwrap().as_ref(), b"# saved\n");
    let replay = client
        .get(&endpoint)
        .header("origin", ORIGIN)
        .send()
        .await
        .unwrap();
    // Existing consumed/expired grant semantics stay 403, not a new status-code contract.
    assert_eq!(replay.status(), StatusCode::FORBIDDEN);
    assert_eq!(replay.headers()["access-control-allow-origin"], ORIGIN);
    fixture.stop().await;
}
