use model::{Context, Lifecycle, Limits, Request, Runtime, ServerInfo, VERSION};

use super::*;

#[tokio::test]
async fn workspace_subscription_waits_for_busy_jobs_before_publishing_its_snapshot() {
    let runtime = Arc::new(Runtime::new(ServerInfo {
        server_id: "server-test".to_owned(),
        instance_id: "instance".to_owned(),
        version: None,
        listen: "127.0.0.1:7316".to_owned(),
        lifecycle: Lifecycle::Ready,
        protocol: VERSION,
        capabilities: Vec::new(),
        implemented_capabilities: Vec::new(),
        features: Vec::new(),
        limits: Limits::default(),
    }));
    let state = crate::dispatch::State {
        runtime: runtime.clone(),
        directory: Some(Arc::new(Mutex::new(directory()))),
        push_tokens: None,
        daemon: None,
        workspace_labels: None,
        workspace_automation: None,
        workspace_state: None,
        worktrees: None,
        workspace_recovery: None,
        github_projects: None,
        creations: model::creation::Creations::default(),
        session_events: model::session::SessionEvents::default(),
        has_agent_execution: false,
        has_terminals: false,
        has_git_fetch: false,
    };
    let held = runtime.jobs.clone().acquire_owned().await.unwrap();
    let (outbound, mut receiver) = model::outbound::Outbound::new();
    let mut connection = crate::connection::Connection::default();
    {
        let context = Context {
            request: Request {
                id: "directory-request".to_owned(),
                method: "workspace.list.request".to_owned(),
                params: json!({"subscribe":{}}),
            },
            runtime: &runtime,
            outbound: &outbound,
            available_subscriptions: 16,
        };
        let task = crate::connection::directory::subscribe(context, &state, &mut connection);
        tokio::pin!(task);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(30), &mut task)
                .await
                .is_err()
        );
        assert!(receiver.try_recv().is_err());
        drop(held);
        task.await.unwrap();
    }
    let queued = receiver.try_recv().unwrap();
    let model::outbound::Frame::Text(text) = &queued.message else {
        panic!("expected a workspace snapshot");
    };
    let response: Value = serde_json::from_str(text).unwrap();
    assert_eq!(response["type"], "response");
    assert_eq!(response["request_id"], "directory-request");
    assert!(response["result"]["subscriptionId"].is_string());
    assert_eq!(connection.len(), 1);
    drop(connection);
    runtime.cancellation.cancel();
    runtime.tasks.close();
    runtime.tasks.wait().await;
}
