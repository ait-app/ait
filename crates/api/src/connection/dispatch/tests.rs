use model::Request;
use model::outbound::Frame;
use serde_json::{Value, json};

use super::*;
use crate::{Api, Services};

const OWNED_METHODS: &[&str] = &[
    "relay.status.request",
    "schedule.list.request",
    "browser.host.register.request",
    "voice.abort.request",
    "connection.ping",
    "agent.skills.get_status.request",
    "agent.list.request",
    "terminal.list.request",
];

fn api() -> Api {
    Api::new(
        "127.0.0.1:7316".parse().unwrap(),
        "test-server".to_owned(),
        "test-instance".to_owned(),
        "in-process-test-token-at-least-32-characters".into(),
        Services::default(),
    )
    .unwrap()
}

fn context<'a>(method: &str, state: &'a Shared, outbound: &'a Outbound) -> Context<'a> {
    Context {
        request: Request {
            id: "request-1".to_owned(),
            method: method.to_owned(),
            params: json!({"nonce":"alive"}),
        },
        runtime: state,
        outbound,
        available_subscriptions: 16,
    }
}

async fn try_handlers(
    context: &mut Option<Context<'_>>,
    state: &Shared,
    subscriptions: &mut ConnectionSubscriptions,
) -> Result<(), QueueError> {
    super::super::workspace_creation::request(context, state).await?;
    super::super::workspace_archive::request(context, state).await?;
    crate::relay_rpc::request(context, state).await?;
    schedule::dispatch::dispatch(context, &state.schedule).await?;
    browser::dispatch::dispatch(context, &state.browser, &mut subscriptions.browser)?;
    voice::dispatch::dispatch(context, &state.voice, &mut subscriptions.voice).await?;
    assert!(
        metadata::dispatch::dispatch(context, &state.metadata, &mut subscriptions.metadata)
            .await?
            .is_none()
    );
    filesystem::dispatch::dispatch(context, &state.filesystem, &mut subscriptions.filesystem)
        .await?;
    assert!(
        provider::dispatch::dispatch(context, &state.provider, &mut subscriptions.provider)
            .await?
            .is_none()
    );
    terminal::dispatch::dispatch(context, &state.terminal, &mut subscriptions.terminals).await
}

fn decode(frame: Frame) -> Value {
    let Frame::Text(text) = frame else {
        panic!("expected a JSON response");
    };
    serde_json::from_str(&text).unwrap()
}

#[tokio::test]
async fn unmatched_context_survives_every_handler_and_empty_context_is_a_noop() {
    let api = api();
    let (outbound, mut receiver) = Outbound::new();
    let mut subscriptions = ConnectionSubscriptions::default();
    let mut pending = Some(context("unknown.request", &api.shared, &outbound));
    pending.as_mut().unwrap().available_subscriptions = 0;

    try_handlers(&mut pending, &api.shared, &mut subscriptions)
        .await
        .unwrap();

    let context = pending.as_ref().unwrap();
    assert_eq!(context.request.id, "request-1");
    assert_eq!(context.request.method, "unknown.request");
    assert_eq!(context.request.params, json!({"nonce":"alive"}));
    assert_eq!(context.available_subscriptions, 0);
    assert_eq!(subscriptions.len(), 0);
    assert!(receiver.try_recv().is_err());

    try_handlers(&mut None, &api.shared, &mut subscriptions)
        .await
        .unwrap();
    assert!(receiver.try_recv().is_err());
}

#[tokio::test]
async fn each_owner_consumes_once_and_later_handlers_leave_the_response_alone() {
    let api = api();
    for &method in OWNED_METHODS {
        let (outbound, mut receiver) = Outbound::new();
        let mut subscriptions = ConnectionSubscriptions::default();
        let mut pending = Some(context(method, &api.shared, &outbound));

        try_handlers(&mut pending, &api.shared, &mut subscriptions)
            .await
            .unwrap();

        assert!(pending.is_none(), "{method}");
        let response = decode(receiver.try_recv().unwrap().message);
        assert_eq!(response["request_id"], "request-1", "{method}");
        if ["connection.ping", "relay.status.request"].contains(&method) {
            assert_eq!(response["type"], "response", "{method}");
        } else if method == "terminal.list.request" {
            assert_eq!(response["code"], "not_implemented", "{method}");
        } else {
            assert_eq!(response["code"], "unsupported_capability", "{method}");
        }
        assert!(receiver.try_recv().is_err(), "{method}");
    }
}

#[tokio::test]
async fn response_delivery_failure_consumes_the_request_and_propagates() {
    let api = api();
    for &method in OWNED_METHODS {
        let (outbound, receiver) = Outbound::new();
        drop(receiver);
        let mut subscriptions = ConnectionSubscriptions::default();
        let mut pending = Some(context(method, &api.shared, &outbound));

        let result = try_handlers(&mut pending, &api.shared, &mut subscriptions).await;

        assert!(matches!(result, Err(QueueError::Full)), "{method}");
        assert!(pending.is_none(), "{method}");
    }
}

#[tokio::test]
async fn request_chain_handles_late_owners_and_replies_once_when_nobody_handles_it() {
    let api = api();
    for method in OWNED_METHODS.iter().copied().chain(["unknown.request"]) {
        let (outbound, mut receiver) = Outbound::new();
        let mut subscriptions = ConnectionSubscriptions::default();

        request(
            context(method, &api.shared, &outbound),
            &api.shared,
            &mut subscriptions,
        )
        .await
        .unwrap();

        let response = decode(receiver.try_recv().unwrap().message);
        assert_eq!(response["request_id"], "request-1", "{method}");
        if ["unknown.request", "terminal.list.request"].contains(&method) {
            assert_eq!(response["code"], "not_implemented");
        } else if ["connection.ping", "relay.status.request"].contains(&method) {
            assert_eq!(response["type"], "response", "{method}");
        } else {
            assert_eq!(response["code"], "unsupported_capability", "{method}");
        }
        assert!(receiver.try_recv().is_err(), "{method}");
    }
}

#[tokio::test]
async fn status_unsubscribe_and_release_finish_removing_the_consumed_subscription() {
    let api = api();
    for method in ["server.status.unsubscribe", "subscription.release.request"] {
        let (outbound, mut receiver) = Outbound::new();
        let mut subscriptions = ConnectionSubscriptions::default();
        request(
            context("server.status.subscribe", &api.shared, &outbound),
            &api.shared,
            &mut subscriptions,
        )
        .await
        .unwrap();
        let response = decode(receiver.try_recv().unwrap().message);
        let id = response["result"]["subscription_id"].clone();
        assert_eq!(
            decode(receiver.try_recv().unwrap().message)["type"],
            "status"
        );
        assert_eq!(subscriptions.len(), 1);
        let mut context = context(method, &api.shared, &outbound);
        context.request.params = if method == "server.status.unsubscribe" {
            json!({"subscription_id":id})
        } else {
            json!({"subscriptionId":id})
        };

        request(context, &api.shared, &mut subscriptions)
            .await
            .unwrap();

        assert_eq!(subscriptions.len(), 0);
        assert_eq!(
            decode(receiver.try_recv().unwrap().message)["type"],
            "response"
        );
        assert!(receiver.try_recv().is_err());
    }
}

#[tokio::test]
async fn workspace_composition_consumes_matching_requests_before_capability_handlers() {
    let api = api();
    for (method, params) in [
        ("workspace.create.request", json!({"agent":{}})),
        ("workspace.archive.request", json!({})),
        ("project.remove.request", json!({})),
        ("workspace.worktree.archive.request", json!({})),
    ] {
        let (outbound, mut receiver) = Outbound::new();
        let mut subscriptions = ConnectionSubscriptions::default();
        let mut pending = Some(context(method, &api.shared, &outbound));
        pending.as_mut().unwrap().request.params = params;

        try_handlers(&mut pending, &api.shared, &mut subscriptions)
            .await
            .unwrap();

        assert!(pending.is_none(), "{method}");
        let response = decode(receiver.try_recv().unwrap().message);
        assert_eq!(response["request_id"], "request-1");
        assert_eq!(response["type"], "error");
        assert!(receiver.try_recv().is_err());
    }
}

#[tokio::test]
async fn metadata_followup_finishes_before_the_consumed_request_returns() {
    let api = api();
    for (method, params, expected) in [
        (
            "subscription.release.request",
            json!({"subscriptionId":"subscription-1"}),
            "response",
        ),
        ("daemon.get_status.request", json!({}), "error"),
    ] {
        let (outbound, mut receiver) = Outbound::new();
        let mut subscriptions = ConnectionSubscriptions::default();
        let mut context = context(method, &api.shared, &outbound);
        context.request.params = params;

        request(context, &api.shared, &mut subscriptions)
            .await
            .unwrap();

        let response = decode(receiver.try_recv().unwrap().message);
        assert_eq!(response["type"], expected);
        assert_eq!(response["request_id"], "request-1");
        if expected == "response" {
            assert_eq!(response["result"]["subscriptionId"], "subscription-1");
        } else {
            assert_eq!(response["code"], "unsupported_capability");
        }
        assert!(receiver.try_recv().is_err());
    }
}
