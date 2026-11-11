use std::sync::Arc;

use model::outbound::{Frame, Outbound, Queued};
use model::{Context, Lifecycle, Limits, Request, Runtime, ServerInfo};
use serde_json::{Value, json};
use tokio::sync::mpsc::Receiver;

use super::super::subscribe;
use crate::connection::Connection;
use crate::dispatch::State;

fn state() -> State {
    State {
        runtime: Arc::new(Runtime::new(ServerInfo {
            server_id: "test".to_owned(),
            version: None,
            instance_id: "instance".to_owned(),
            listen: "127.0.0.1:0".to_owned(),
            lifecycle: Lifecycle::Ready,
            protocol: model::VERSION,
            capabilities: Vec::new(),
            implemented_capabilities: Vec::new(),
            features: Vec::new(),
            limits: Limits::default(),
        })),
        push_tokens: None,
        daemon: None,
        directory: None,
        workspace_labels: None,
        workspace_automation: None,
        workspace_state: None,
        worktrees: None,
        workspace_recovery: None,
        github_projects: None,
        creations: model::creation::Creations::default(),
        session_events: model::session::SessionEvents::default(),
        has_agent_execution: true,
        has_terminals: false,
        has_git_fetch: false,
    }
}

fn receive(receiver: &mut Receiver<Queued>) -> Value {
    let queued = receiver.try_recv().unwrap();
    let Frame::Text(text) = &queued.message else {
        panic!("expected JSON frame");
    };
    serde_json::from_str(text).unwrap()
}

fn connect(state: &State, secret_questions: bool) -> (Connection, Receiver<Queued>, String) {
    let mut connection = Connection::new(state.session_events.connect());
    let (outbound, mut receiver) = Outbound::new();
    subscribe(
        Context {
            request: Request {
                id: "subscribe".to_owned(),
                method: "session.events.set_subscription.request".to_owned(),
                params: json!({"events": ["agent_permission_request", "agent_permission_resolved"]}),
            },
            runtime: &state.runtime,
            outbound: &outbound,
            available_subscriptions: 1,
            secret_questions,
        },
        state,
        &mut connection,
    )
    .unwrap();
    let id = receive(&mut receiver)["result"]["subscriptionId"]
        .as_str()
        .unwrap()
        .to_owned();
    (connection, receiver, id)
}

#[tokio::test]
async fn permission_event_projection_is_connection_specific_and_preserves_cancellation() {
    use domain::session::protocol::SessionEventKind;

    let state = state();
    let (_legacy, mut legacy, legacy_id) = connect(&state, false);
    let (_capable, mut capable, capable_id) = connect(&state, true);
    let secret = json!({
        "id": "dialog", "provider": "omp", "name": "extension_ui", "kind": "question",
        "title": "sensitive-title", "description": "sensitive-description",
        "input": {"questions": [{"secret": false}, {"secret": true, "initialValue": "sensitive-prefill"}]},
        "detail": {"input": "sensitive-detail"}, "metadata": {"value": "sensitive-metadata"},
        "suggestions": [{"value": "sensitive-suggestion"}],
        "actions": [{"id": "allow", "label": "Submit", "behavior": "allow"}]
    });
    let publication = json!({"agentId": "agent", "request": secret});
    state
        .session_events
        .publish(SessionEventKind::AgentPermissionRequest, &publication);
    let legacy_event = receive(&mut legacy);
    let capable_event = receive(&mut capable);
    assert_eq!(legacy_event["method"], "agent_permission_request");
    assert_eq!(legacy_event["params"]["agentId"], "agent");
    assert_eq!(legacy_event["params"]["subscriptionId"], legacy_id);
    let request = &legacy_event["params"]["request"];
    assert_eq!(request["id"], "dialog");
    assert_eq!(request["provider"], "omp");
    assert_eq!(request["kind"], "tool");
    assert!(request["description"].as_str().unwrap().contains("Upgrade"));
    assert_eq!(
        request["actions"],
        json!([{"id": "deny", "label": "Cancel", "behavior": "deny"}])
    );
    for field in ["input", "detail", "metadata", "suggestions"] {
        assert!(request.get(field).is_none(), "unexpected field: {field}");
    }
    assert!(!legacy_event.to_string().contains("sensitive-"));
    assert_eq!(capable_event["params"]["subscriptionId"], capable_id);
    assert_eq!(capable_event["params"]["request"], secret);
    assert_eq!(publication["request"], secret);

    let mut public = secret;
    public["input"]["questions"][1]["secret"] = json!(false);
    state.session_events.publish(
        SessionEventKind::AgentPermissionRequest,
        &json!({"agentId": "agent", "request": public}),
    );
    assert_eq!(receive(&mut legacy)["params"]["request"], public);
    assert_eq!(receive(&mut capable)["params"]["request"], public);

    state.session_events.publish(
        SessionEventKind::AgentPermissionResolved,
        &json!({"agentId": "agent", "requestId": "dialog", "response": {"behavior": "deny"}}),
    );
    for receiver in [&mut legacy, &mut capable] {
        let resolved = receive(receiver);
        assert_eq!(resolved["method"], "agent_permission_resolved");
        assert_eq!(resolved["params"]["requestId"], "dialog");
        assert_eq!(resolved["params"]["response"], json!({"behavior": "deny"}));
    }
}
