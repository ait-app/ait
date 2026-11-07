use model::outbound::{Frame, Outbound};
use model::{Lifecycle, Limits, Request, Runtime, ServerInfo};

use super::*;

fn resources() -> (Connector, Runtime) {
    let connector = Connector::new(
        "127.0.0.1:7316".parse().unwrap(),
        "relay-server".to_owned(),
        "relay-instance".to_owned(),
    );
    let runtime = Runtime::new(ServerInfo {
        server_id: "other-server".to_owned(),
        version: None,
        instance_id: "other-instance".to_owned(),
        listen: "127.0.0.1:7316".to_owned(),
        lifecycle: Lifecycle::Ready,
        protocol: model::VERSION,
        capabilities: Vec::new(),
        implemented_capabilities: Vec::new(),
        features: Vec::new(),
        limits: Limits::default(),
    });
    (connector, runtime)
}

fn context<'a>(
    method: &str,
    params: Value,
    runtime: &'a Runtime,
    outbound: &'a Outbound,
) -> Context<'a> {
    Context {
        request: Request {
            id: "relay-request".to_owned(),
            method: method.to_owned(),
            params,
        },
        runtime,
        outbound,
        available_subscriptions: 0,
    }
}

fn decode(frame: Frame) -> Value {
    let Frame::Text(text) = frame else {
        panic!("expected a JSON response");
    };
    serde_json::from_str(&text).unwrap()
}

#[tokio::test]
async fn unrelated_requests_keep_their_context_and_consumed_requests_are_skipped() {
    let (connector, runtime) = resources();
    let (outbound, mut receiver) = Outbound::new();
    let mut pending = Some(context("connection.ping", json!({}), &runtime, &outbound));

    assert!(matches!(
        request(&mut pending, &connector, None),
        Err(DispatchError::NotImplemented)
    ));
    assert_eq!(pending.as_ref().unwrap().request.method, "connection.ping");
    assert!(receiver.try_recv().is_err());
    request(&mut None, &connector, None).unwrap();
    assert!(receiver.try_recv().is_err());
}

#[tokio::test]
async fn status_and_stop_reply_once_with_only_the_connectors_public_identity_and_state() {
    let (connector, runtime) = resources();
    let (outbound, mut receiver) = Outbound::new();
    for method in ["relay.status.request", "relay.stop.request"] {
        let mut pending = Some(context(method, json!({}), &runtime, &outbound));
        request(&mut pending, &connector, None).unwrap();

        assert!(pending.is_none());
        assert_eq!(
            decode(receiver.try_recv().unwrap().message),
            json!({
                "type":"response",
                "request_id":"relay-request",
                "result":{
                    "serverId":"relay-server",
                    "instanceId":"relay-instance",
                    "platform":std::env::consts::OS,
                    "management":{"mode":"external"},
                    "status":{"connecting":false,"online":false,"epoch":null,"error":null}
                }
            })
        );
        request(&mut pending, &connector, None).unwrap();
        assert!(receiver.try_recv().is_err());
    }
}

#[tokio::test]
async fn start_never_opens_a_relay_control_connection() {
    let (connector, runtime) = resources();
    let (outbound, mut receiver) = Outbound::new();
    let grant = json!({"center_url":"https://example.invalid/api", "control_ticket":"a".repeat(64),
        "node_session_id":"00000000-0000-0000-0000-000000000001"});
    for (management, code) in [
        (None, "not_implemented"),
        (Some(json!({"mode":"managed"})), "relay_managed"),
    ] {
        let mut pending = Some(context(
            "relay.start.request",
            grant.clone(),
            &runtime,
            &outbound,
        ));
        request(&mut pending, &connector, management).unwrap();

        assert!(pending.is_none());
        let response = decode(receiver.try_recv().unwrap().message);
        assert_eq!(response["type"], "error");
        assert_eq!(response["request_id"], "relay-request");
        assert_eq!(response["code"], code);
        assert!(!response.to_string().contains("control_ticket"));
    }
}

#[tokio::test]
async fn delivery_failure_consumes_the_handled_request_and_stops_dispatch() {
    let (connector, runtime) = resources();
    let (outbound, receiver) = Outbound::new();
    drop(receiver);
    let mut pending = Some(context(
        "relay.status.request",
        json!({}),
        &runtime,
        &outbound,
    ));

    assert!(matches!(
        request(&mut pending, &connector, None),
        Err(DispatchError::Delivery(_))
    ));
    assert!(pending.is_none());
    assert!(outbound.failure().is_cancelled());
}
