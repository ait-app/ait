use super::*;

#[tokio::test]
async fn quota_reads_use_the_live_agent_account_and_validate_the_rpc_shape() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.program.with_extension("cwd"),
        fixture.cwd.to_str().unwrap(),
    )
    .unwrap();
    let (execution, _) = worker(&fixture);
    let host = execution
        .execute("provider.usage.list.request", json!({}))
        .await
        .unwrap();
    assert_eq!(host["providers"][0]["accountLabel"], "private@example.test");
    let mut agents = Vec::new();
    for (account, percent) in [("first@example.test", "17"), ("second@example.test", "83")] {
        let created = execution
            .execute(
                "agent.create.request",
                json!({
            "idempotencyKey":account,"config":{"provider":"codex","cwd":fixture.cwd},
            "env":{"AIT_TEST_USAGE_ACCOUNT":account,"AIT_TEST_USAGE_PERCENT":percent}}),
            )
            .await
            .unwrap();
        agents.push(created["agentId"].as_str().unwrap().to_owned());
        let report = execution
            .execute(
                "provider.usage.list.request",
                json!({"agentId":created["agentId"],"forceRefresh":true}),
            )
            .await
            .unwrap();
        assert_eq!(report["providers"][0]["accountLabel"], account);
        assert_eq!(
            report["providers"][0]["windows"][0]["usedPct"],
            percent.parse::<u64>().unwrap()
        );
        assert_eq!(report["providers"].as_array().unwrap().len(), 1);
    }
    let host_again = execution
        .execute("provider.usage.list.request", json!({}))
        .await
        .unwrap();
    assert_eq!(host_again["providers"], host["providers"]);
    let refreshed = execution
        .execute(
            "provider.usage.list.request",
            json!({"providerId":"codex","forceRefresh":true}),
        )
        .await
        .unwrap();
    assert_eq!(refreshed["providers"].as_array().unwrap().len(), 1);
    assert_eq!(
        refreshed["providers"][0]["accountLabel"],
        "private@example.test"
    );
    assert_eq!(
        execution
            .execute(
                "provider.usage.list.request",
                json!({"providerId":"unknown"})
            )
            .await,
        Err(ErrorCode::UnsupportedCapability)
    );
    for params in [
        json!({"agentId":42}),
        json!({"forceRefresh":"yes"}),
        json!({"providerId":true}),
        json!({"providerId":""}),
    ] {
        assert_eq!(
            execution
                .execute("provider.usage.list.request", params)
                .await,
            Err(ErrorCode::InvalidMessage)
        );
    }
    assert_eq!(
        execution
            .execute("provider.usage.list.request", json!({"agentId":"unknown"}))
            .await,
        Err(ErrorCode::AgentNotFound)
    );
    assert_eq!(
        execution
            .execute("provider.usage.list.request", json!({"unknown":true}))
            .await,
        Err(ErrorCode::UnsupportedCapability)
    );
    execution.shutdown().await.unwrap();
    let (restarted, _) = worker(&fixture);
    let report = restarted
        .execute("provider.usage.list.request", json!({"agentId":agents[0]}))
        .await
        .unwrap();
    assert_eq!(report["providers"][0]["status"], "unavailable");
    assert!(report["providers"][0].get("accountLabel").is_none());
    restarted.shutdown().await.unwrap();
}
