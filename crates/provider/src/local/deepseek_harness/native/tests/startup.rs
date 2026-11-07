use super::*;
use crate::local::deepseek_harness::native::runtime::Runtime;
use serde_json::json;
use std::time::{Duration, Instant};

#[tokio::test]
async fn cold_native_startup_has_its_own_budget_without_extending_connected_controls() {
    let mut fixture = fixture::Fixture::new().await;
    fixture.client.deadline = Duration::from_millis(500);
    let mut environment: BTreeMap<String, String> = fixture
        .client
        .environment
        .entries()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
    environment.insert("AIT_DSH_FIXTURE_START_DELAY".to_owned(), "1".to_owned());
    fixture.client.environment = AgentEnvironment::try_from(environment).unwrap();
    let started = Instant::now();
    let mut runtime = tokio::time::timeout(
        Duration::from_secs(5),
        Runtime::open(&fixture.client, &fixture.spec.cwd),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(started.elapsed() >= Duration::from_secs(1));
    fixture.fail("fixture/slow", "timeout");
    assert!(
        tokio::time::timeout(
            Duration::from_millis(1500),
            runtime.api.call("fixture/slow", json!({}))
        )
        .await
        .unwrap()
        .is_err()
    );
    runtime.close().await.unwrap();
}
