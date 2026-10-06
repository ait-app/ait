//! Slow discovery cannot delay metadata or timeline observation on the execution lane.

use super::*;

struct DiscoveryGate(std::path::PathBuf);

impl DiscoveryGate {
    fn new(fixture: &Fixture) -> Self {
        fixture.mode("delayed-discovery");
        Self(fixture.cwd.clone())
    }

    async fn entered(&self) {
        tokio::time::timeout(Duration::from_secs(3), async {
            while !self.0.join("discovery-entered").exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("offline discovery entered its gate");
    }

    fn release(&self) {
        std::fs::write(self.0.join("release-discovery"), "").unwrap();
    }
}

impl Drop for DiscoveryGate {
    fn drop(&mut self) {
        let _ = std::fs::write(self.0.join("release-discovery"), "");
    }
}

fn discover(execution: &AgentExecution, fixture: &Fixture) -> tokio::task::JoinHandle<Value> {
    let execution = execution.clone();
    let cwd = fixture.cwd.clone();
    tokio::spawn(async move {
        execution
            .execute("provider.snapshot.get.request", json!({"cwd":cwd}))
            .await
            .unwrap()
    })
}

fn discovery_count(fixture: &Fixture) -> usize {
    fixture
        .requests()
        .iter()
        .filter(|request| request["method"] == "model/list")
        .count()
}

#[tokio::test]
async fn blocked_discovery_keeps_metadata_timeline_and_subscriptions_responsive() {
    let fixture = Fixture::new();
    let (execution, _) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    let gate = DiscoveryGate::new(&fixture);
    let discovery = discover(&execution, &fixture);
    gate.entered().await;
    let mut peer = Peer::new(&execution);
    let responsive = tokio::time::timeout(Duration::from_secs(2), async {
        let agent = execution
            .execute("agent.get.request", json!({"agentId":id}))
            .await
            .unwrap();
        assert_eq!(agent["agent"]["id"], id);
        let page = execution
            .execute("agent.timeline.get.request", json!({"agentId":id}))
            .await
            .unwrap();
        assert!(page["entries"].is_array());
        let subscription = peer.subscribe(json!({"agentIds":[id]}), 8).await;
        assert!(subscription["result"]["subscriptionId"].is_string());
    })
    .await;
    assert!(
        responsive.is_ok(),
        "discovery blocked an independent execution request"
    );
    assert!(!discovery.is_finished());
    gate.release();
    assert_eq!(discovery.await.unwrap()["entries"][0]["status"], "ready");
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn cancelled_discovery_caller_preserves_single_flight_cache_and_conditional_reads() {
    let fixture = Fixture::new();
    let (execution, _) = worker(&fixture);
    let gate = DiscoveryGate::new(&fixture);
    let cancelled = discover(&execution, &fixture);
    gate.entered().await;
    cancelled.abort();
    assert!(cancelled.await.unwrap_err().is_cancelled());
    let waiting = discover(&execution, &fixture);
    gate.release();
    let snapshot = waiting.await.unwrap();
    assert_eq!(snapshot["entries"][0]["status"], "ready");
    let unchanged = execution
        .execute(
            "provider.snapshot.get.request",
            json!({
                "cwd":fixture.cwd,"ifNoneMatch":snapshot["snapshotHash"]
            }),
        )
        .await
        .unwrap();
    assert_eq!(unchanged["notModified"], true);
    assert_eq!(discovery_count(&fixture), 1);
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn discovery_queue_is_bounded_and_shutdown_drains_accepted_requests() {
    let fixture = Fixture::new();
    let (execution, _) = worker(&fixture);
    let gate = DiscoveryGate::new(&fixture);
    let discovery = discover(&execution, &fixture);
    gate.entered().await;
    let mut replies = Vec::new();
    for _ in 0..64 {
        let (reply, receiver) = oneshot::channel();
        execution
            .0
            .catalog
            .try_send(crate::service::agent_execution::catalog::Request {
                method: "provider.snapshot.get.request".to_owned(),
                params: json!({"cwd":fixture.cwd}),
                reply,
            })
            .unwrap();
        replies.push(receiver);
    }
    assert_eq!(
        execution
            .execute("provider.snapshot.get.request", json!({"cwd":fixture.cwd}))
            .await,
        Err(ErrorCode::CatalogBusy)
    );
    let stopping = execution.clone();
    let shutdown = tokio::spawn(async move { stopping.shutdown().await });
    execution.0.catalog_shutdown.cancelled().await;
    assert_eq!(
        execution
            .execute("provider.snapshot.get.request", json!({"cwd":fixture.cwd}))
            .await,
        Err(ErrorCode::AgentIo)
    );
    assert!(!shutdown.is_finished());
    gate.release();
    assert_eq!(discovery.await.unwrap()["entries"][0]["status"], "ready");
    for reply in replies {
        assert_eq!(
            reply.await.unwrap().unwrap()["entries"][0]["status"],
            "ready"
        );
    }
    shutdown.await.unwrap().unwrap();
    assert_eq!(discovery_count(&fixture), 1);
    assert!(execution.0.thread.lock().unwrap().is_none());
}

#[tokio::test]
async fn failed_discovery_can_refresh_without_poisoning_the_execution_lane() {
    let fixture = Fixture::new();
    let (execution, _) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    fixture.mode("error");
    let failed = discover(&execution, &fixture).await.unwrap();
    assert_eq!(failed["entries"][0]["status"], "error");
    assert_eq!(
        execution
            .execute("agent.get.request", json!({"agentId":created["agentId"]}))
            .await
            .unwrap()["agent"]["id"],
        created["agentId"]
    );
    fixture.mode("normal");
    let connection = execution.events().connect();
    let received = Arc::new(Mutex::new(Vec::new()));
    let sink = received.clone();
    let events = connection
        .subscribe(
            serde_json::from_value(json!({"events":["providers_snapshot_update"]})).unwrap(),
            Arc::new(move |kind, value| {
                sink.lock().unwrap().push((kind, value));
                Ok(())
            }),
        )
        .unwrap();
    events.activate().unwrap();
    let refreshed = execution
        .execute(
            "provider.snapshot.refresh.request",
            json!({"cwd":fixture.cwd}),
        )
        .await
        .unwrap();
    assert_eq!(refreshed["acknowledged"], true);
    assert_eq!(received.lock().unwrap().len(), 1);
    assert_eq!(
        received.lock().unwrap()[0].1["entries"][0]["status"],
        "ready"
    );
    assert_eq!(
        discover(&execution, &fixture).await.unwrap()["entries"][0]["status"],
        "ready"
    );
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn execution_loop_exit_closes_discovery_even_while_its_sender_is_retained() {
    let fixture = Fixture::new();
    let (execution, _) = worker(&fixture);
    let gate = DiscoveryGate::new(&fixture);
    let discovery = discover(&execution, &fixture);
    gate.entered().await;
    let (reply, receiver) = oneshot::channel();
    execution
        .0
        .sender
        .send(Command::Shutdown(reply))
        .await
        .unwrap();
    receiver.await.unwrap().unwrap();
    tokio::time::timeout(
        Duration::from_secs(2),
        execution.0.catalog_shutdown.cancelled(),
    )
    .await
    .expect("execution exit closes catalog admission without caller assistance");
    gate.release();
    discovery.await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), execution.shutdown())
        .await
        .expect("both loops exit although the execution handle retains catalog sender")
        .unwrap();
}

#[tokio::test]
async fn catalog_response_slots_remain_bounded_until_their_owners_are_dropped() {
    let fixture = Fixture::new();
    let (execution, _) = worker(&fixture);
    let gate = DiscoveryGate::new(&fixture);
    let discovery = discover(&execution, &fixture);
    gate.entered().await;
    let mut responses = Vec::new();
    for _ in 0..63 {
        responses.push(
            execution
                .admit_catalog("provider.snapshot.get.request", json!({"cwd":fixture.cwd}))
                .unwrap(),
        );
    }
    assert!(matches!(
        execution.admit_catalog("provider.snapshot.get.request", json!({"cwd":fixture.cwd})),
        Err(ErrorCode::CatalogBusy)
    ));
    gate.release();
    discovery.await.unwrap();
    for response in &mut responses {
        assert_eq!(
            response.receive().await.unwrap()["entries"][0]["status"],
            "ready"
        );
    }
    let retained = execution
        .admit_catalog("provider.snapshot.get.request", json!({"cwd":fixture.cwd}))
        .unwrap();
    assert!(matches!(
        execution.admit_catalog("provider.snapshot.get.request", json!({"cwd":fixture.cwd})),
        Err(ErrorCode::CatalogBusy)
    ));
    drop(responses);
    drop(retained);
    assert_eq!(
        discover(&execution, &fixture).await.unwrap()["entries"][0]["status"],
        "ready"
    );
    assert_eq!(discovery_count(&fixture), 1);
    execution.shutdown().await.unwrap();
}
