use super::*;

pub(super) struct Capture {
    pub(super) path: String,
    version: reqwest::Version,
    authorization: String,
    topic: Option<String>,
    pub(super) body: Vec<u8>,
}

pub(super) struct MockServer(tokio::task::JoinHandle<()>);
impl Drop for MockServer {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub(super) async fn mock_server(
    status: StatusCode,
    body: Value,
) -> (String, tokio::sync::mpsc::Receiver<Capture>, MockServer) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (sent, received) = tokio::sync::mpsc::channel(8);
    let router = axum::Router::new().fallback(move |request: axum::extract::Request| {
        let sent = sent.clone();
        let body = body.clone();
        async move {
            let capture = Capture {
                path: request.uri().path().to_owned(),
                version: request.version(),
                authorization: request
                    .headers()
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("")
                    .to_owned(),
                topic: request
                    .headers()
                    .get("apns-topic")
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned),
                body: axum::body::to_bytes(request.into_body(), 8192)
                    .await
                    .unwrap()
                    .to_vec(),
            };
            sent.send(capture).await.unwrap();
            (status, axum::Json(body))
        }
    });
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (format!("http://{address}"), received, MockServer(server))
}

#[tokio::test]
async fn native_transports_send_authenticated_http_requests_and_classify_rejection() {
    let (url, mut requests, _server) = mock_server(StatusCode::OK, json!({})).await;
    let adapter = NativePush {
        client: Client::builder().http2_prior_knowledge().build().unwrap(),
        apple: Some(auth::tests::apple()),
        google: Some(auth::tests::google(format!("{url}/token"))),
        endpoints: Endpoints {
            apns: url.clone(),
            sandbox: url.clone(),
            fcm: url,
        },
    };
    let notification = notification();
    adapter
        .apns(&notification, "abcd", "dev.ait.mobile", "production")
        .await
        .unwrap();
    let apple = requests.recv().await.unwrap();
    assert_eq!(apple.version, reqwest::Version::HTTP_2);
    assert_eq!(apple.path, "/3/device/abcd");
    assert_eq!(apple.topic.as_deref(), Some("dev.ait.mobile"));
    assert!(apple.authorization.starts_with("Bearer ey"));
    assert_eq!(
        serde_json::from_slice::<Value>(&apple.body).unwrap()["body"],
        notification.data
    );
    adapter
        .fcm(&notification, "native-fcm-token")
        .await
        .unwrap();
    let google = requests.recv().await.unwrap();
    assert_eq!(google.path, "/v1/projects/test-project/messages:send");
    assert_eq!(google.authorization, "Bearer mock-access-token");
    assert_eq!(
        serde_json::from_slice::<Value>(&google.body).unwrap()["message"]["token"],
        "native-fcm-token"
    );
    assert_eq!(
        adapter
            .apns(&notification, "abcd", "other.app", "production")
            .await,
        Err(DeliveryError::Rejected)
    );
    assert_eq!(
        adapter
            .apns(&notification, "ab/cd", "dev.ait.mobile", "production")
            .await,
        Err(DeliveryError::Rejected)
    );
    assert_eq!(
        adapter
            .apns(&notification, "abcd", "dev.ait.mobile", "unknown")
            .await,
        Err(DeliveryError::Rejected)
    );
}

fn notification() -> Notification {
    Notification {
        title: "Agent finished".to_owned(),
        body: "Done".to_owned(),
        data: json!({"serverId":"host","workspaceId":"workspace","agentId":"agent"}),
    }
}

#[test]
fn missing_provider_configuration_keeps_the_daemon_startable() {
    let transport = NativePush::from_environment(|_| None).unwrap();
    assert!(format!("{transport:?}").contains("apns_configured: false"));
    assert!(format!("{transport:?}").contains("fcm_configured: false"));
    let partial = NativePush::from_environment(|name| {
        (name == "AIT_APNS_KEY_ID").then(|| "fake-test-key".to_owned())
    });
    assert!(partial.is_err());
}

#[test]
fn native_payloads_preserve_the_receivers_routing_contract() {
    let notification = notification();
    let apple = apns_payload(&notification);
    assert_eq!(apple["aps"]["alert"]["title"], "Agent finished");
    assert_eq!(apple["aps"]["sound"], "default");
    assert_eq!(apple["body"], notification.data);
    let google = fcm_payload(&notification, "test-token");
    assert_eq!(google["message"]["token"], "test-token");
    assert_eq!(google["message"]["android"]["priority"], "HIGH");
    assert_eq!(
        serde_json::from_str::<Value>(google["message"]["data"]["body"].as_str().unwrap()).unwrap(),
        notification.data
    );
}

#[test]
fn provider_responses_only_revoke_explicitly_invalid_device_tokens() {
    assert_eq!(
        classify_apns(StatusCode::GONE, "Unregistered", 60),
        Err(DeliveryError::Unregistered)
    );
    assert_eq!(
        classify_apns(StatusCode::BAD_REQUEST, "BadDeviceToken", 60),
        Err(DeliveryError::Unregistered)
    );
    assert_eq!(
        classify_apns(StatusCode::FORBIDDEN, "InvalidProviderToken", 60),
        Err(DeliveryError::Rejected)
    );
    assert_eq!(
        classify_apns(StatusCode::TOO_MANY_REQUESTS, "TooManyRequests", 120),
        Err(DeliveryError::Retryable(120))
    );
    assert_eq!(
        classify_apns(StatusCode::SERVICE_UNAVAILABLE, "Shutdown", 60),
        Err(DeliveryError::Retryable(0))
    );
    assert_eq!(
        classify_fcm(
            StatusCode::NOT_FOUND,
            &json!({"error":{"details":[{"@type":"type.googleapis.com/google.firebase.fcm.v1.FcmError","errorCode":"UNREGISTERED"}]}}),
            60
        ),
        Err(DeliveryError::Unregistered)
    );
    assert_eq!(
        classify_fcm(
            StatusCode::BAD_REQUEST,
            &json!({"error":{"status":"INVALID_ARGUMENT"}}),
            60
        ),
        Err(DeliveryError::Rejected)
    );
    assert_eq!(
        classify_fcm(StatusCode::NOT_FOUND, &json!({}), 60),
        Err(DeliveryError::Rejected)
    );
    assert_eq!(
        classify_fcm(StatusCode::TOO_MANY_REQUESTS, &json!({}), 90),
        Err(DeliveryError::Retryable(90))
    );
    assert_eq!(
        classify_fcm(StatusCode::BAD_GATEWAY, &json!({}), 60),
        Err(DeliveryError::Retryable(0))
    );
}

#[tokio::test]
async fn unavailable_credentials_and_legacy_expo_tokens_fail_without_disclosing_secrets() {
    let adapter = NativePush {
        client: Client::new(),
        apple: None,
        google: None,
        endpoints: Endpoints::default(),
    };
    let notification = notification();
    assert_eq!(
        adapter
            .send(&notification, "ExponentPushToken[test-secret]")
            .await,
        Err(DeliveryError::Unregistered)
    );
    assert_eq!(
        adapter
            .send(&notification, r#"{"provider":"fcm","token":"test-secret"}"#)
            .await,
        Err(DeliveryError::Unconfigured)
    );
    assert_eq!(adapter.send(&notification, r#"{"provider":"apns","token":"abcd","bundleId":"dev.ait.mobile","environment":"production"}"#).await, Err(DeliveryError::Unconfigured));
    assert!(!format!("{adapter:?}").contains("test-secret"));
}
