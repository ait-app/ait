use super::*;

async fn persisted(fixture: &Fixture) -> String {
    let (original, _) = worker(fixture);
    let agent = create(&original, fixture).await;
    let id = agent["agentId"].as_str().unwrap().to_owned();
    original
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"stored"}),
        )
        .await
        .unwrap();
    original
        .execute("agent.finish.wait.request", json!({"agentId":id}))
        .await
        .unwrap();
    original.shutdown().await.unwrap();
    id
}

async fn cached(execution: &AgentExecution, id: &str, options: Value) -> Value {
    let mut params = json!({"agentId":id,"direction":"tail","limit":1,"allowCached":true});
    for (key, value) in options.as_object().unwrap() {
        params[key] = value.clone();
    }
    tokio::time::timeout(
        Duration::from_secs(5),
        execution.execute("agent.timeline.get.request", params),
    )
    .await
    .unwrap()
    .unwrap()
}

#[tokio::test]
async fn stored_tail_returns_before_native_history_and_older_pages_remain_available() {
    let fixture = Fixture::new();
    let id = persisted(&fixture).await;
    let gate = Gate::new(Operation::History);
    let execution = gated(&fixture, &gate);

    let tail = cached(&execution, &id, json!({})).await;
    assert_eq!(tail["historyNeedsRefresh"], true);
    assert_eq!(tail["entries"].as_array().unwrap().len(), 1);
    assert_eq!(tail["entries"][0]["item"]["text"], "Echo: stored");
    assert_eq!(tail["hasOlder"], true);
    assert_eq!(gate.calls.load(Ordering::SeqCst), 0);

    let hydration = {
        let execution = execution.clone();
        let id = id.clone();
        tokio::spawn(async move {
            execution
                .execute(
                    "agent.timeline.get.request",
                    json!({"agentId":id,"limit":1}),
                )
                .await
        })
    };
    entered(&gate).await;
    let older = cached(
        &execution,
        &id,
        json!({"direction":"before","cursor":tail["startCursor"]}),
    )
    .await;
    assert_eq!(older["entries"][0]["item"]["text"], "stored");
    assert_eq!(older["historyNeedsRefresh"], true);
    assert!(!hydration.is_finished());
    gate.release.add_permits(1);
    hydration.await.unwrap().unwrap();
    let fresh = cached(&execution, &id, json!({})).await;
    assert_eq!(fresh["historyNeedsRefresh"], false);
    assert_eq!(fresh["epoch"], tail["epoch"]);
    assert_eq!(gate.calls.load(Ordering::SeqCst), 1);
    assert!(
        !fixture
            .requests()
            .iter()
            .any(|request| request["method"] == "thread/resume")
    );
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn empty_history_still_waits_for_native_import_instead_of_reporting_a_cached_empty_tail() {
    let fixture = Fixture::new();
    let (original, _) = worker(&fixture);
    let agent = create(&original, &fixture).await;
    original.shutdown().await.unwrap();
    let gate = Gate::new(Operation::History);
    let execution = gated(&fixture, &gate);
    let first = {
        let execution = execution.clone();
        tokio::spawn(async move {
            execution
                .execute(
                    "agent.timeline.get.request",
                    json!({"agentId":agent["agentId"],"allowCached":true,"limit":40}),
                )
                .await
        })
    };
    entered(&gate).await;
    assert!(!first.is_finished());
    gate.release.add_permits(1);
    let response = first.await.unwrap().unwrap();
    assert_eq!(response["historyNeedsRefresh"], false);
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn failed_native_revalidation_retains_the_committed_tail() {
    let fixture = Fixture::new();
    let id = persisted(&fixture).await;
    fixture.mode("error");
    let gate = Gate::new(Operation::History);
    gate.release.add_permits(1);
    let execution = gated(&fixture, &gate);
    let tail = cached(&execution, &id, json!({})).await;

    assert_eq!(
        execution
            .execute("agent.timeline.get.request", json!({"agentId":id}))
            .await,
        Err(ErrorCode::AgentIo)
    );
    let retained = cached(&execution, &id, json!({})).await;
    assert_eq!(retained["entries"], tail["entries"]);
    assert_eq!(retained["historyNeedsRefresh"], true);
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn cached_requests_validate_the_complete_request_before_native_work() {
    let fixture = Fixture::new();
    let id = persisted(&fixture).await;
    let gate = Gate::new(Operation::History);
    let execution = gated(&fixture, &gate);
    assert_eq!(
        execution
            .execute(
                "agent.timeline.get.request",
                json!({"agentId":id,"allowCached":true,"unknown":true}),
            )
            .await,
        Err(ErrorCode::InvalidMessage)
    );
    assert_eq!(gate.calls.load(Ordering::SeqCst), 0);
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn pending_native_replacement_is_excluded_from_cached_reads() {
    let fixture = Fixture::new();
    let id = persisted(&fixture).await;
    let (execution, registry) = worker(&fixture);
    let mut record = registry.get(&id).unwrap().unwrap();
    record
        .persistence
        .as_mut()
        .unwrap()
        .metadata
        .get_or_insert_with(Default::default)
        .insert("aitTimelineReplacement".to_owned(), json!(true));
    registry.upsert(&record).unwrap();
    let reader = execution.0.template.lock().unwrap().fork(None);
    assert!(
        !reader
            .can_read_cached_timeline(
                "agent.timeline.get.request",
                &json!({"agentId":id,"allowCached":true}),
            )
            .unwrap()
    );
    execution.shutdown().await.unwrap();
}
