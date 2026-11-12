use std::time::Duration;

use super::*;

fn codex_peer(fixture: &Fixture) -> (AgentExecution, Peer) {
    let (execution, _) = worker(fixture);
    let peer = Peer::new(&execution);
    (execution, peer)
}

/// Whether the Codex fake received any request, which it logs from its first line of input.
fn launched(fixture: &Fixture) -> bool {
    fixture.cwd.join("native-requests.jsonl").exists()
}

fn parameters(fixture: &Fixture) -> Value {
    json!({
        "config":{"provider":"codex","cwd":fixture.cwd},
        "idempotencyKey":"subscribed-codex",
        "subscribe":true
    })
}

async fn request(peer: &Peer, params: Value, budget: usize) {
    Connection::create(
        Context {
            request: Request {
                id: "create-codex".to_owned(),
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
            assert_eq!(value["request_id"], "create-codex");
            return value;
        }
    }
}

#[tokio::test]
async fn subscribed_creation_waits_for_busy_foreground_jobs_before_launching() {
    let fixture = Fixture::new();
    let (execution, mut peer) = codex_peer(&fixture);
    // Foreground work occupies all shared permits, even across Workspaces.
    let permit = peer
        .state
        .runtime
        .jobs
        .clone()
        .acquire_many_owned(3)
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
        assert!(!launched(&fixture));
        drop(permit);
        tokio::time::timeout(Duration::from_secs(10), pending)
            .await
            .unwrap();
    }
    let created = response(&mut peer);
    assert_eq!(created["type"], "response");
    assert_eq!(created["result"]["agent"]["provider"], "codex");
    assert_eq!(created["result"]["creation"]["phase"], "completed");
    assert!(launched(&fixture), "the probe observes the native launch");
    assert_eq!(peer.state.runtime.jobs.available_permits(), 3);
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn draining_cancels_queued_creation_without_launching() {
    let fixture = Fixture::new();
    let (execution, mut peer) = codex_peer(&fixture);
    let _permit = peer
        .state
        .runtime
        .jobs
        .clone()
        .acquire_many_owned(3)
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
    assert!(!launched(&fixture));
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn exhausted_subscription_capacity_rejects_creation_before_launching() {
    let fixture = Fixture::new();
    let (execution, mut peer) = codex_peer(&fixture);
    request(&peer, parameters(&fixture), 0).await;
    assert_eq!(response(&mut peer)["code"], "resource_exhausted");
    assert!(!launched(&fixture));
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn unsubscribed_creation_does_not_need_foreground_job_capacity() {
    let fixture = Fixture::new();
    let (execution, mut peer) = codex_peer(&fixture);
    let _permit = peer
        .state
        .runtime
        .jobs
        .clone()
        .acquire_many_owned(3)
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
async fn failed_native_startup_returns_agent_io_and_keeps_runtime_ready() {
    let fixture = Fixture::new();
    let (execution, mut peer) = codex_peer(&fixture);
    fixture.mode("malformed");
    request(&peer, parameters(&fixture), 1).await;
    assert_eq!(response(&mut peer)["code"], "agent_io");
    assert_eq!(peer.state.runtime.info().lifecycle, Lifecycle::Ready);
    assert_eq!(peer.state.runtime.jobs.available_permits(), 3);
    execution.shutdown().await.unwrap();
}
