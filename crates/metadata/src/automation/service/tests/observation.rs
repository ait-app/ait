use super::*;
use model::outbound::{Frame, Outbound, Queued};
use model::{Context, Lifecycle, Limits, Request, ServerInfo};
use serde_json::{Value, json};
use tokio::sync::mpsc::Receiver;

fn state() -> crate::dispatch::State {
    crate::dispatch::State {
        runtime: Arc::new(model::Runtime::new(ServerInfo {
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
        workspace_automation: Some(Arc::new(Mutex::new(service().0))),
        workspace_state: None,
        worktrees: None,
        workspace_recovery: None,
        github_projects: None,
        creations: model::creation::Creations::default(),
        session_events: model::session::SessionEvents::default(),
        has_agent_execution: false,
        has_terminals: false,
        has_git_fetch: false,
    }
}

fn receive(receiver: &mut Receiver<Queued>) -> Value {
    let queued = receiver.try_recv().unwrap();
    let Frame::Text(text) = &queued.message else {
        panic!("expected JSON response");
    };
    serde_json::from_str(text).unwrap()
}

#[tokio::test]
async fn setup_observation_bootstraps_then_filters_and_releases_with_its_physical_connection() {
    let state = state();
    let mut connection = crate::connection::Connection::new(state.session_events.connect());
    let mut sibling = crate::connection::Connection::new(state.session_events.connect());
    let (outbound, mut receiver) = Outbound::new();
    let context = Context {
        request: Request {
            id: "request".to_owned(),
            method: "workspace.setup.subscribe.request".to_owned(),
            params: json!({"workspaceId":"snapshot"}),
        },
        runtime: &state.runtime,
        outbound: &outbound,
        available_subscriptions: 1,
    };
    crate::automation::connection::subscribe(context, &state, &mut connection)
        .await
        .unwrap();
    let response = receive(&mut receiver);
    assert_eq!(response["result"]["snapshot"]["status"], "completed");
    assert_eq!(response["result"]["snapshot"]["revision"], 1);
    let subscription = response["result"]["subscriptionId"].as_str().unwrap();
    sibling.release(subscription);
    state.session_events.publish(
        domain::session::protocol::SessionEventKind::WorkspaceSetupProgress,
        &json!({"workspaceId":"other","revision":2}),
    );
    assert!(receiver.try_recv().is_err());
    state.session_events.publish(
        domain::session::protocol::SessionEventKind::WorkspaceSetupProgress,
        &json!({"workspaceId":"snapshot","revision":3}),
    );
    let event = receive(&mut receiver);
    assert_eq!(event["params"]["subscriptionId"], subscription);
    assert_eq!(event["params"]["revision"], 3);
    connection.release(subscription);
    state.session_events.publish(
        domain::session::protocol::SessionEventKind::WorkspaceSetupProgress,
        &json!({"workspaceId":"snapshot","revision":4}),
    );
    assert!(receiver.try_recv().is_err());
    assert!(connection.is_empty());
}

#[tokio::test]
async fn setup_observation_respects_the_shared_subscription_budget() {
    let state = state();
    let mut connection = crate::connection::Connection::new(state.session_events.connect());
    let (outbound, mut receiver) = Outbound::new();
    let context = Context {
        request: Request {
            id: "request".to_owned(),
            method: "workspace.setup.subscribe.request".to_owned(),
            params: json!({"workspaceId":"snapshot"}),
        },
        runtime: &state.runtime,
        outbound: &outbound,
        available_subscriptions: 0,
    };
    crate::automation::connection::subscribe(context, &state, &mut connection)
        .await
        .unwrap();
    assert_eq!(receive(&mut receiver)["code"], "resource_exhausted");
    assert!(connection.is_empty());
}
