use domain::workspace::worktrees::{
    CreatedWorktreeWorkspace, DirectoryGit, WorktreeCreation, WorktreeCreationError,
};
use model::{Context, Lifecycle, Limits, Request, Runtime, ServerInfo, VERSION};

use super::*;

fn runtime() -> Arc<Runtime> {
    Arc::new(Runtime::new(ServerInfo {
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
    }))
}

fn state(runtime: Arc<Runtime>, directory: Directory) -> crate::dispatch::State {
    crate::dispatch::State {
        runtime,
        directory: Some(Arc::new(Mutex::new(directory))),
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
    }
}

#[tokio::test]
async fn workspace_subscription_waits_for_busy_jobs_before_publishing_its_snapshot() {
    let runtime = runtime();
    let state = state(runtime.clone(), directory());
    let held = runtime.jobs.clone().acquire_many_owned(3).await.unwrap();
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
        let task =
            crate::directory::connection::directory::subscribe(context, &state, &mut connection);
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

/// Provisioning fake that parks inside `create` until the test releases it.
#[derive(Debug)]
struct ParkedProvisioning {
    entered: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    release: Mutex<std::sync::mpsc::Receiver<()>>,
}

impl model::workspace::worktrees::WorktreeProvisioning for ParkedProvisioning {
    fn prepare_directory(
        &self,
        _cwd: &str,
        _intent: &DirectoryGit,
    ) -> Result<(), WorktreeCreationError> {
        Ok(())
    }

    fn archive(&self, _workspace_id: &str, _timestamp: &str) -> Result<(), WorktreeCreationError> {
        Ok(())
    }

    fn create(
        &self,
        _input: &WorktreeCreation,
        _timestamp: &str,
    ) -> Result<CreatedWorktreeWorkspace, WorktreeCreationError> {
        if let Some(entered) = self.entered.lock().unwrap().take() {
            entered.send(()).unwrap();
        }
        self.release.lock().unwrap().recv().unwrap();
        Err(WorktreeCreationError {
            code: "unknown",
            message: "parked".to_owned(),
        })
    }
}

#[tokio::test]
async fn worktree_creation_does_not_hold_the_directory_lock_while_provisioning() {
    let runtime = runtime();
    let (entered, entered_signal) = tokio::sync::oneshot::channel();
    let (release, released) = std::sync::mpsc::channel();
    let directory = directory().with_worktrees(Arc::new(ParkedProvisioning {
        entered: Mutex::new(Some(entered)),
        release: Mutex::new(released),
    }));
    let state = state(runtime.clone(), directory);
    let (outbound, mut receiver) = model::outbound::Outbound::new();
    let context = Context {
        request: Request {
            id: "create-request".to_owned(),
            method: "workspace.create.request".to_owned(),
            params: json!({"source":{"kind":"worktree","cwd":"/tmp/alpha"}}),
        },
        runtime: &runtime,
        outbound: &outbound,
        available_subscriptions: 16,
    };
    let shared = state.directory.clone().unwrap();
    let probe = async move {
        entered_signal.await.unwrap();
        let unlocked = shared.try_lock().is_ok();
        release.send(()).unwrap();
        unlocked
    };
    let (created, unlocked) = tokio::join!(
        crate::directory::connection::workspace_creation::create(context, &state),
        probe
    );
    created.unwrap();
    assert!(
        unlocked,
        "directory lock must be free while provisioning runs"
    );
    let model::outbound::Frame::Text(text) = &receiver.try_recv().unwrap().message else {
        panic!("expected a creation response");
    };
    let response: Value = serde_json::from_str(text).unwrap();
    assert_eq!(response["request_id"], "create-request");
    assert_eq!(response["result"]["error"], "parked");
    runtime.cancellation.cancel();
    runtime.tasks.close();
    runtime.tasks.wait().await;
}
