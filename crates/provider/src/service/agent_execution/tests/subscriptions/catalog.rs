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
            .execute(
                "provider.models.list.request",
                json!({"provider":"codex","cwd":cwd}),
            )
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
    assert!(discovery.await.unwrap()["models"].is_array());
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
    assert!(waiting.await.unwrap()["models"].is_array());
    let snapshot = execution
        .execute("provider.snapshot.get.request", json!({"cwd":fixture.cwd}))
        .await
        .unwrap();
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
    for _ in 0..63 {
        replies.push(
            execution
                .admit_catalog(
                    "provider.models.list.request",
                    json!({"provider":"codex","cwd":fixture.cwd}),
                )
                .unwrap(),
        );
    }
    assert_eq!(
        execution
            .execute("provider.snapshot.get.request", json!({"cwd":fixture.cwd}))
            .await,
        Err(ErrorCode::CatalogBusy)
    );
    let stopping = execution.clone();
    let shutdown = tokio::spawn(async move { stopping.shutdown().await });
    execution.0.cancellation.cancelled().await;
    assert_eq!(
        execution
            .execute("provider.snapshot.get.request", json!({"cwd":fixture.cwd}))
            .await,
        Err(ErrorCode::AgentIo)
    );
    assert!(!shutdown.is_finished());
    gate.release();
    assert!(discovery.await.unwrap()["models"].is_array());
    for mut reply in replies {
        assert!(reply.receive().await.unwrap()["models"].is_array());
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
    assert_eq!(failed["error"], "Provider discovery failed");
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
    assert!(discover(&execution, &fixture).await.unwrap()["models"].is_array());
    assert_eq!(
        received.lock().unwrap().last().unwrap().1["entries"][0]["status"],
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
    tokio::time::timeout(Duration::from_secs(2), execution.0.cancellation.cancelled())
        .await
        .expect("execution exit closes catalog admission without caller assistance");
    gate.release();
    discovery.await.unwrap();
    receiver.await.unwrap().unwrap();
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
                .admit_catalog(
                    "provider.models.list.request",
                    json!({"provider":"codex","cwd":fixture.cwd}),
                )
                .unwrap(),
        );
    }
    assert!(matches!(
        execution.admit_catalog(
            "provider.models.list.request",
            json!({"provider":"codex","cwd":fixture.cwd})
        ),
        Err(ErrorCode::CatalogBusy)
    ));
    gate.release();
    discovery.await.unwrap();
    for response in &mut responses {
        assert!(response.receive().await.unwrap()["models"].is_array());
    }
    let retained = execution
        .admit_catalog(
            "provider.models.list.request",
            json!({"provider":"codex","cwd":fixture.cwd}),
        )
        .unwrap();
    assert!(matches!(
        execution.admit_catalog(
            "provider.models.list.request",
            json!({"provider":"codex","cwd":fixture.cwd})
        ),
        Err(ErrorCode::CatalogBusy)
    ));
    drop(responses);
    drop(retained);
    assert!(discover(&execution, &fixture).await.unwrap()["models"].is_array());
    assert_eq!(discovery_count(&fixture), 1);
    execution.shutdown().await.unwrap();
}
