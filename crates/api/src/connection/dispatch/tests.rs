use model::outbound::Frame;
use model::{DispatchError, Request};
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
    match super::super::workspace_creation::request(context, state).await {
        Ok(()) => {}
        Err(DispatchError::NotImplemented) => {
            Context::assert_unhandled(context, "api::workspace_creation");
        }
        Err(DispatchError::Delivery(error)) => return Err(error),
    }
    match super::super::workspace_archive::request(context, state).await {
        Ok(()) => {}
        Err(DispatchError::NotImplemented) => {
            Context::assert_unhandled(context, "api::workspace_archive");
        }
        Err(DispatchError::Delivery(error)) => return Err(error),
    }
    match crate::relay_rpc::request(context, state).await {
        Ok(()) => {}
        Err(DispatchError::NotImplemented) => Context::assert_unhandled(context, "api::relay"),
        Err(DispatchError::Delivery(error)) => return Err(error),
    }
    match schedule::dispatch::dispatch(context, &state.schedule).await {
        Ok(()) => {}
        Err(DispatchError::NotImplemented) => Context::assert_unhandled(context, "schedule"),
        Err(DispatchError::Delivery(error)) => return Err(error),
    }
    match browser::dispatch::dispatch(context, &state.browser, &mut subscriptions.browser) {
        Ok(()) => {}
        Err(DispatchError::NotImplemented) => Context::assert_unhandled(context, "browser"),
        Err(DispatchError::Delivery(error)) => return Err(error),
    }
    match voice::dispatch::dispatch(context, &state.voice, &mut subscriptions.voice).await {
        Ok(()) => {}
        Err(DispatchError::NotImplemented) => Context::assert_unhandled(context, "voice"),
        Err(DispatchError::Delivery(error)) => return Err(error),
    }
    match metadata::dispatch::dispatch(context, &state.metadata, &mut subscriptions.metadata)
        .await
        .map(|completion| assert!(completion.is_none()))
    {
        Ok(()) => {}
        Err(DispatchError::NotImplemented) => Context::assert_unhandled(context, "metadata"),
        Err(DispatchError::Delivery(error)) => return Err(error),
    }
    match filesystem::dispatch::dispatch(context, &state.filesystem, &mut subscriptions.filesystem)
        .await
    {
        Ok(()) => {}
        Err(DispatchError::NotImplemented) => Context::assert_unhandled(context, "filesystem"),
        Err(DispatchError::Delivery(error)) => return Err(error),
    }
    match provider::dispatch::dispatch(context, &state.provider, &mut subscriptions.provider)
        .await
        .map(|completion| assert!(completion.is_none()))
    {
        Ok(()) => {}
        Err(DispatchError::NotImplemented) => Context::assert_unhandled(context, "provider"),
        Err(DispatchError::Delivery(error)) => return Err(error),
    }
    match terminal::dispatch::dispatch(context, &state.terminal, &mut subscriptions.terminals).await
    {
        Ok(()) => {}
        Err(DispatchError::NotImplemented) => Context::assert_unhandled(context, "terminal"),
        Err(DispatchError::Delivery(error)) => return Err(error),
    }
    Ok(())
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
async fn request_chain_stops_on_delivery_failure_from_each_owner() {
    let api = api();
    for &method in OWNED_METHODS {
        let (outbound, receiver) = Outbound::new();
        drop(receiver);
        let mut subscriptions = ConnectionSubscriptions::default();

        let result = request(
            context(method, &api.shared, &outbound),
            &api.shared,
            &mut subscriptions,
        )
        .await;

        assert!(matches!(result, Err(QueueError::Full)), "{method}");
        assert!(outbound.failure().is_cancelled(), "{method}");
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

#[tokio::test]
async fn unmatched_owners_return_not_implemented_without_consuming_or_delivering() {
    let api = api();
    let (outbound, mut receiver) = Outbound::new();
    let mut subscriptions = ConnectionSubscriptions::default();
    for method in [
        "unknown.request",
        "checkout.future.request",
        "agent.future.request",
        "workspace.future.request",
        "push.register",
        "session.heartbeat",
        "terminal.input",
        "browser.automation.execute.response",
        "voice.audio.chunk",
        "voice.audio.played",
        "dictation.stream.chunk",
    ] {
        let mut pending = Some(context(method, &api.shared, &outbound));
        let results = [
            super::super::workspace_creation::request(&mut pending, &api.shared).await,
            super::super::workspace_archive::request(&mut pending, &api.shared).await,
            crate::relay_rpc::request(&mut pending, &api.shared).await,
            schedule::dispatch::dispatch(&mut pending, &api.shared.schedule).await,
            browser::dispatch::dispatch(
                &mut pending,
                &api.shared.browser,
                &mut subscriptions.browser,
            ),
            voice::dispatch::dispatch(&mut pending, &api.shared.voice, &mut subscriptions.voice)
                .await,
            metadata::dispatch::dispatch(
                &mut pending,
                &api.shared.metadata,
                &mut subscriptions.metadata,
            )
            .await
            .map(|_| ()),
            filesystem::dispatch::dispatch(
                &mut pending,
                &api.shared.filesystem,
                &mut subscriptions.filesystem,
            )
            .await,
            provider::dispatch::dispatch(
                &mut pending,
                &api.shared.provider,
                &mut subscriptions.provider,
            )
            .await
            .map(|_| ()),
            terminal::dispatch::dispatch(
                &mut pending,
                &api.shared.terminal,
                &mut subscriptions.terminals,
            )
            .await,
        ];
        for result in results {
            assert!(
                matches!(result, Err(DispatchError::NotImplemented)),
                "{method}: {result:?}"
            );
        }
        let context = pending.as_ref().unwrap();
        assert_eq!(context.request.id, "request-1");
        assert_eq!(context.request.method, method);
        assert_eq!(context.request.params, json!({"nonce":"alive"}));
        assert_eq!(context.available_subscriptions, 16);
        assert!(receiver.try_recv().is_err());
    }
}

#[tokio::test]
async fn every_declared_request_reaches_its_owners_consuming_branch() {
    let owners = [
        schedule::capabilities::implemented_capabilities().collect::<Vec<_>>(),
        browser::capabilities::implemented_capabilities().collect(),
        voice::capabilities::implemented_capabilities().collect(),
        metadata::capabilities::implemented_capabilities()
            .chain(["server.status.unsubscribe"])
            .collect(),
        filesystem::capabilities::implemented_capabilities().collect(),
        provider::capabilities::implemented_capabilities().collect(),
        terminal::capabilities::implemented_capabilities().collect(),
    ];
    for (owner, methods) in owners.into_iter().enumerate() {
        for method in methods {
            let entry = super::super::validation::lookup(method).unwrap();
            if entry.kind != protocol::methods::InboundKind::Request {
                continue;
            }
            let api = api();
            let (outbound, _receiver) = Outbound::new();
            let mut subscriptions = ConnectionSubscriptions::default();
            let mut pending = Some(context(method, &api.shared, &outbound));
            let result = match owner {
                0 => schedule::dispatch::dispatch(&mut pending, &api.shared.schedule).await,
                1 => browser::dispatch::dispatch(
                    &mut pending,
                    &api.shared.browser,
                    &mut subscriptions.browser,
                ),
                2 => {
                    voice::dispatch::dispatch(
                        &mut pending,
                        &api.shared.voice,
                        &mut subscriptions.voice,
                    )
                    .await
                }
                3 => metadata::dispatch::dispatch(
                    &mut pending,
                    &api.shared.metadata,
                    &mut subscriptions.metadata,
                )
                .await
                .map(|_| ()),
                4 => {
                    filesystem::dispatch::dispatch(
                        &mut pending,
                        &api.shared.filesystem,
                        &mut subscriptions.filesystem,
                    )
                    .await
                }
                5 => provider::dispatch::dispatch(
                    &mut pending,
                    &api.shared.provider,
                    &mut subscriptions.provider,
                )
                .await
                .map(|_| ()),
                6 => {
                    terminal::dispatch::dispatch(
                        &mut pending,
                        &api.shared.terminal,
                        &mut subscriptions.terminals,
                    )
                    .await
                }
                _ => unreachable!(),
            };
            assert!(result.is_ok(), "{method}: {result:?}");
            assert!(pending.is_none(), "{method} remained unhandled");
        }
    }
}

#[tokio::test]
async fn workspace_creation_without_an_agent_continues_to_metadata() {
    let api = api();
    for agent in [None, Some(Value::Null)] {
        let (outbound, mut receiver) = Outbound::new();
        let mut pending = Some(context("workspace.create.request", &api.shared, &outbound));
        if let Some(agent) = agent {
            pending.as_mut().unwrap().request.params["agent"] = agent;
        }
        let params = pending.as_ref().unwrap().request.params.clone();
        let result = super::super::workspace_creation::request(&mut pending, &api.shared).await;
        assert!(matches!(result, Err(DispatchError::NotImplemented)));
        assert_eq!(pending.as_ref().unwrap().request.params, params);
        let mut subscriptions = ConnectionSubscriptions::default();
        metadata::dispatch::dispatch(
            &mut pending,
            &api.shared.metadata,
            &mut subscriptions.metadata,
        )
        .await
        .unwrap();
        assert!(pending.is_none());
        assert_eq!(
            decode(receiver.try_recv().unwrap().message)["code"],
            "unsupported_capability"
        );
        assert!(receiver.try_recv().is_err());
    }
}
