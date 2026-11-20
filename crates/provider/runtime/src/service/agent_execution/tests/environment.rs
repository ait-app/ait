use super::*;

#[tokio::test]
async fn create_environment_reaches_only_its_codex_process_and_is_not_persisted_in_receipts() {
    let fixture = Fixture::new();
    std::fs::write(fixture.cwd.join("capture-environment"), "").unwrap();
    let receipt_path = fixture.root.path().join("creations.json");
    let creations = persistence::storage::creation::open(&receipt_path).unwrap();
    let (execution, registry) = worker_with_creations(&fixture, creations);
    let request = json!({"idempotencyKey":"environment-create","config":{"provider":"codex","cwd":fixture.cwd},"env":{"AIT_TEST_AGENT_ENV":"private-fixture-value"}});
    let created = execution
        .execute("agent.create.request", request.clone())
        .await
        .unwrap();
    let retried = execution
        .execute("agent.create.request", request.clone())
        .await
        .unwrap();
    assert_eq!(created["agentId"], retried["agentId"]);
    assert!(!created.to_string().contains("private-fixture-value"));
    assert!(
        !std::fs::read(receipt_path.with_extension("sqlite3"))
            .unwrap()
            .windows(b"private-fixture-value".len())
            .any(|bytes| bytes == b"private-fixture-value")
    );
    assert!(
        !serde_json::to_string(&registry.list().unwrap())
            .unwrap()
            .contains("private-fixture-value")
    );
    assert!(
        environment_values(&fixture)
            .iter()
            .any(|value| value == "private-fixture-value")
    );
    let mut changed = request;
    changed["env"]["AIT_TEST_AGENT_ENV"] = json!("different");
    assert!(
        execution
            .execute("agent.create.request", changed)
            .await
            .is_err()
    );
    std::fs::write(fixture.cwd.join("native-environment.jsonl"), "").unwrap();
    create(&execution, &fixture).await;
    assert!(environment_values(&fixture).iter().all(Value::is_null));
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn invalid_environment_is_rejected_before_creation_receipts_or_native_launch() {
    let fixture = Fixture::new();
    let receipts = fixture.root.path().join("creations.json");
    let creations = persistence::storage::creation::open(&receipts).unwrap();
    let (execution, registry) = worker_with_creations(&fixture, creations);
    let database = receipts.with_extension("sqlite3");
    let initial = std::fs::read(&database).unwrap();
    for env in [
        json!({"BAD=NAME":"value"}),
        json!({"VALUE":"with\u{0}nul"}),
        json!({"VALUE":42}),
    ] {
        assert_eq!(
            execution
                .execute(
                    "agent.create.request",
                    json!({"config":{"provider":"codex","cwd":fixture.cwd},"env":env})
                )
                .await,
            Err(ErrorCode::InvalidMessage)
        );
    }
    assert_eq!(
        registry.list().unwrap(),
        [] as [domain::agent_runtime::PersistedAgentRuntimeRecord; 0]
    );
    assert_eq!(std::fs::read(database).unwrap(), initial);
    assert!(!fixture.cwd.join("native-requests.jsonl").exists());
    execution.shutdown().await.unwrap();
}

fn environment_values(fixture: &Fixture) -> Vec<Value> {
    std::fs::read_to_string(fixture.cwd.join("native-environment.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap()["value"].clone())
        .collect()
}
