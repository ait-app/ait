use std::sync::{Arc, Mutex};

use metadata::labels::service::WorkspaceLabels;
use persistence::storage::registry::FileBackedWorkspaceRegistry;
use persistence::storage::workspace_labels::FileWorkspaceLabelStore;

use super::*;

#[tokio::test]
async fn label_subscription_waits_for_busy_jobs_and_keeps_its_connection_owner() {
    let root = tempfile::tempdir().unwrap();
    let store = FileWorkspaceLabelStore::new(
        root.path(),
        FileBackedWorkspaceRegistry::new(root.path().join("workspaces.json")),
    );
    let mut api = api();
    let metadata = Arc::get_mut(&mut Arc::get_mut(&mut api.shared).unwrap().metadata).unwrap();
    metadata.workspace_labels = Some(Arc::new(Mutex::new(
        WorkspaceLabels::new(Box::new(store)).unwrap(),
    )));
    let held = api.shared.jobs.clone().acquire_many_owned(3).await.unwrap();
    let (outbound, mut receiver) = Outbound::new();
    let mut subscriptions = ConnectionSubscriptions::default();
    let mut request = context("workspace.label.list.request", &api.shared, &outbound);
    request.request.params = json!({"subscribe":{}});
    {
        let mut pending = Some(request);
        let task = try_handlers(&mut pending, &api.shared, &mut subscriptions);
        tokio::pin!(task);

        // Before this fix the admitted subscription immediately returned ResourceExhausted.
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
    let Frame::Text(text) = &queued.message else {
        panic!("expected a label snapshot");
    };
    let response: Value = serde_json::from_str(text).unwrap();
    assert_eq!(response["type"], "response");
    assert_eq!(response["request_id"], "request-1");
    assert!(response["result"]["subscriptionId"].is_string());
    assert_eq!(subscriptions.metadata.len(), 1);
    assert_eq!(api.shared.jobs.available_permits(), 3);
}
