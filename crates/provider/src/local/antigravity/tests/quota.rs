use super::*;
use crate::service::agent_manager::{AgentManager, AgentRegistration};

#[tokio::test]
async fn logged_in_antigravity_host_and_session_quotas_use_native_command() {
    let (directory, client, spec) = fixture();
    let mut manager = AgentManager::new(Box::new(
        persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry::new(
            directory.path().join("agents.json"),
        ),
    ));
    manager.register_client(Box::new(client)).unwrap();
    let report = manager.usage(None, Some(PROVIDER), true).await.unwrap();
    let host = &report["providers"][0];
    assert_eq!(host["status"], "available", "{host}");
    assert_eq!(host["windows"].as_array().unwrap().len(), 4);
    assert_eq!(host["windows"][0]["id"], "gemini-weekly");
    assert_eq!(host["windows"][0]["remainingPct"], 75.0);
    assert_eq!(host["windows"][0]["usedPct"], 25.0);
    assert_eq!(host["windows"][0]["label"], "Gemini Models · Weekly");
    assert_eq!(host["windows"][1]["shortLabel"], "5h");
    let query_count = logs(&directory).len();
    let cached = manager.usage(None, Some(PROVIDER), false).await.unwrap();
    assert_eq!(cached["providers"][0], *host);
    assert_eq!(logs(&directory).len(), query_count);
    manager.usage(None, Some(PROVIDER), true).await.unwrap();
    assert_eq!(logs(&directory).len(), query_count + 1);

    let environment = AgentEnvironment::try_from(BTreeMap::from([
        ("AGY_FIXTURE_REMAINING".into(), "0.25".into()),
        (
            "AGY_FIXTURE_LOG".into(),
            directory
                .path()
                .join("requests.jsonl")
                .to_str()
                .unwrap()
                .into(),
        ),
    ]))
    .unwrap();
    manager
        .create_with_environment(
            "quota-agent",
            &spec,
            AgentRegistration::default(),
            &environment,
        )
        .await
        .unwrap();
    let agent = manager
        .usage(Some("quota-agent"), Some(PROVIDER), true)
        .await
        .unwrap();
    assert_eq!(agent["providers"][0]["status"], "available");
    assert_eq!(agent["providers"][0]["windows"][0]["remainingPct"], 25.0);
    let host_again = manager.usage(None, Some(PROVIDER), true).await.unwrap();
    assert_eq!(
        host_again["providers"][0]["windows"][0]["remainingPct"],
        75.0
    );
    manager.close_all().await.unwrap();
    let stopped = manager
        .usage(Some("quota-agent"), Some(PROVIDER), true)
        .await
        .unwrap();
    assert_eq!(stopped["providers"][0]["status"], "unavailable");
    let calls = logs(&directory);
    assert!(calls.iter().all(|call| call.get("input").is_none()));
    let quota_calls: Vec<_> = calls
        .iter()
        .filter(|call| call["args"][0] == "--print")
        .collect();
    assert_eq!(quota_calls.len(), 4);
    assert_eq!(quota_calls[2]["cwd"], spec.cwd);
    for call in quota_calls {
        assert_eq!(
            call["args"],
            json!(["--print", "/usage", "--output-format", "json"])
        );
    }
}

#[tokio::test]
async fn quota_query_failures_are_bounded_and_never_expose_native_error_text() {
    for scenario in [
        "quota-auth-error",
        "quota-error",
        "malformed",
        "oversized",
        "hung",
    ] {
        let (directory, mut client, _) = fixture();
        client.environment = AgentEnvironment::try_from(BTreeMap::from([(
            "AGY_FIXTURE_SCENARIO".into(),
            scenario.into(),
        )]))
        .unwrap();
        if scenario == "hung" {
            client.deadline = Duration::from_millis(100);
        }
        let mut manager = AgentManager::new(Box::new(
            persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry::new(
                directory.path().join("agents.json"),
            ),
        ));
        manager.register_client(Box::new(client)).unwrap();
        let report = tokio::time::timeout(
            Duration::from_secs(3),
            manager.usage(None, Some(PROVIDER), true),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(report["providers"][0]["status"], "error", "{scenario}");
        assert!(!report.to_string().contains("fixture-secret"));
        assert_eq!(report["providers"][0]["windows"], json!([]));
    }
    let (_directory, mut client, _) = fixture();
    client.environment = AgentEnvironment::try_from(BTreeMap::from([(
        "AGY_FIXTURE_SCENARIO".into(),
        "quota-empty".into(),
    )]))
    .unwrap();
    let report = client.usage().await.unwrap();
    assert_eq!(report["status"], "unavailable");
    assert!(
        !report["problem"]["detail"]
            .as_str()
            .unwrap()
            .contains("Sign in")
    );
}
