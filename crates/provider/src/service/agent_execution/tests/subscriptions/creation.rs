use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use crate::local::deepseek_harness::DeepSeekHarnessClient;

use super::*;

fn dsh_peer(fixture: &Fixture) -> (AgentExecution, Peer) {
    let program = fixture.root.path().join("dsh");
    std::fs::write(
        &program,
        include_str!("../../../../local/deepseek_harness/tests/fixtures/acp.cjs"),
    )
    .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    let (execution, _) = worker_with_client(
        fixture,
        model::creation::Creations::default(),
        Box::new(DeepSeekHarnessClient::new(program).with_acp_profile()),
    );
    let peer = Peer::new(&execution);
    (execution, peer)
}

fn parameters(fixture: &Fixture) -> Value {
    json!({
        "config":{"provider":"deepseek-harness","cwd":fixture.cwd},
        "idempotencyKey":"subscribed-dsh",
        "subscribe":true,
        "env":{"ACP_FIXTURE_LOG":fixture.cwd.join("acp-requests.jsonl")}
    })
}

async fn request(peer: &Peer, params: Value, budget: usize) {
    Connection::create(
        Context {
            request: Request {
                id: "create-dsh".to_owned(),
                method: "agent.create.request".to_owned(),
                params,
            },
            runtime: &peer.state.runtime,
            outbound: &peer.outbound,
            available_subscriptions: budget,
        },
        &peer.state,
    )
    .await
    .unwrap();
}

fn response(peer: &mut Peer) -> Value {
    loop {
        let value = peer.next();
        if value["type"] != "event" {
            assert_eq!(value["request_id"], "create-dsh");
            return value;
        }
    }
}

#[tokio::test]
async fn subscribed_dsh_creation_waits_for_busy_foreground_jobs_before_launching() {
    let fixture = Fixture::new();
    let (execution, mut peer) = dsh_peer(&fixture);
    // A slow Diff read occupies this same server-wide permit, even in another Workspace.
    let permit = peer
        .state
        .runtime
        .jobs
        .clone()
        .acquire_owned()
        .await
        .unwrap();
    {
        let pending = request(&peer, parameters(&fixture), 1);
        tokio::pin!(pending);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut pending)
                .await
                .is_err(),
            "creation must wait for temporary job contention"
        );
        assert!(!fixture.cwd.join("acp-requests.jsonl").exists());
        drop(permit);
        tokio::time::timeout(Duration::from_secs(10), pending)
            .await
            .unwrap();
    }
    let created = response(&mut peer);
    assert_eq!(created["type"], "response");
    assert_eq!(created["result"]["agent"]["provider"], "deepseek-harness");
    assert_eq!(created["result"]["creation"]["phase"], "completed");
    assert_eq!(peer.state.runtime.jobs.available_permits(), 1);
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn draining_cancels_queued_dsh_creation_without_launching() {
    let fixture = Fixture::new();
    let (execution, mut peer) = dsh_peer(&fixture);
    let _permit = peer
        .state
        .runtime
        .jobs
        .clone()
        .acquire_owned()
        .await
        .unwrap();
    {
        let pending = request(&peer, parameters(&fixture), 1);
        tokio::pin!(pending);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut pending)
                .await
                .is_err()
        );
        peer.state.runtime.cancellation.cancel();
        tokio::time::timeout(Duration::from_secs(1), pending)
            .await
            .unwrap();
    }
    assert_eq!(response(&mut peer)["code"], "server_draining");
    assert!(!fixture.cwd.join("acp-requests.jsonl").exists());
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn exhausted_subscription_capacity_rejects_dsh_creation_before_launching() {
    let fixture = Fixture::new();
    let (execution, mut peer) = dsh_peer(&fixture);
    request(&peer, parameters(&fixture), 0).await;
    assert_eq!(response(&mut peer)["code"], "resource_exhausted");
    assert!(!fixture.cwd.join("acp-requests.jsonl").exists());
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn unsubscribed_dsh_creation_does_not_need_foreground_job_capacity() {
    let fixture = Fixture::new();
    let (execution, mut peer) = dsh_peer(&fixture);
    let _permit = peer
        .state
        .runtime
        .jobs
        .clone()
        .acquire_owned()
        .await
        .unwrap();
    let mut params = parameters(&fixture);
    params["subscribe"] = json!(false);
    tokio::time::timeout(Duration::from_secs(10), request(&peer, params, 0))
        .await
        .unwrap();
    assert_eq!(response(&mut peer)["type"], "response");
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn failed_dsh_startup_returns_agent_io_and_keeps_runtime_ready() {
    let fixture = Fixture::new();
    let (execution, mut peer) = dsh_peer(&fixture);
    let mut params = parameters(&fixture);
    params["env"]["ACP_FIXTURE_SCENARIO"] = json!("malformed");
    request(&peer, params, 1).await;
    assert_eq!(response(&mut peer)["code"], "agent_io");
    assert_eq!(peer.state.runtime.info().lifecycle, Lifecycle::Ready);
    assert_eq!(peer.state.runtime.jobs.available_permits(), 1);
    execution.shutdown().await.unwrap();
}
