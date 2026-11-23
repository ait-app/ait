use super::*;

use crate::ports::usage_ledger::{
    CallBegin, CallFinish, CallOutcome, CallSource, RequestRecord, RequestUsage, UsageBoundary,
    UsageLedger,
};
use crate::storage::usage_ledger::SqliteUsageLedger;
use crate::test_support::{request, worker_with_ledger};

#[tokio::test]
async fn usage_response_slots_stay_bounded_until_delivery_and_reject_mutations() {
    let fixture = Fixture::new();
    let (execution, _) = worker(&fixture);
    let mut responses = Vec::new();
    for _ in 0..64 {
        let mut response = execution
            .admit_discovery(
                "provider.usage.list.request",
                json!({"providerId":"unknown"}),
            )
            .unwrap();
        assert_eq!(
            response.receive().await,
            Err(ErrorCode::UnsupportedCapability)
        );
        responses.push(response);
    }
    assert!(matches!(
        execution.admit_discovery("provider.usage.list.request", json!({})),
        Err(ErrorCode::CatalogBusy)
    ));
    assert!(matches!(
        execution.admit_discovery("agent.message.send.request", json!({})),
        Err(ErrorCode::UnsupportedCapability)
    ));
    drop(responses.pop());
    let mut admitted = execution
        .admit_discovery(
            "provider.usage.list.request",
            json!({"providerId":"unknown"}),
        )
        .unwrap();
    assert_eq!(
        admitted.receive().await,
        Err(ErrorCode::UnsupportedCapability)
    );
    drop(admitted);
    drop(responses);
    execution.shutdown().await.unwrap();
    assert!(matches!(
        execution.admit_discovery("provider.usage.list.request", json!({})),
        Err(ErrorCode::AgentIo)
    ));
}

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
    assert_eq!(report["providers"], json!([]));
    restarted.shutdown().await.unwrap();
}

#[tokio::test]
async fn missing_provider_binary_is_omitted_from_host_usage_rpc() {
    let fixture = Fixture::new();
    let client = crate::local::codex::CodexClient::new(fixture.root.path().join("missing-codex"));
    let (execution, _) = worker_with_client(
        &fixture,
        model::creation::Creations::default(),
        Box::new(client),
    );
    let response = execution
        .execute("provider.usage.list.request", json!({}))
        .await
        .unwrap();
    assert_eq!(response["providers"], json!([]));
    execution.shutdown().await.unwrap();
}

/// 2025-11-08T00:00:00Z, when the test ledger starts.
const LEDGER_SINCE: i64 = 1_762_560_000_000;
/// One UTC day in milliseconds.
const DAY_MS: i64 = 86_400_000;

/// Ledger under `fixture` with one completed Codex user call an hour after it started.
fn recorded_ledger(fixture: &Fixture) -> Arc<SqliteUsageLedger> {
    let ledger = Arc::new(
        SqliteUsageLedger::open_with_clock(&fixture.root.path().join("data"), || LEDGER_SINCE)
            .unwrap(),
    );
    let started_at = LEDGER_SINCE + 3_600_000;
    ledger.begin_call(CallBegin {
        call_id: "call-1".into(),
        agent_id: "agent-1".into(),
        provider: "codex".into(),
        model: Some("gpt-5".into()),
        source: CallSource::User,
        workspace_id: None,
        started_at,
    });
    ledger.record_request(RequestRecord {
        call_id: "call-1".into(),
        agent_id: "agent-1".into(),
        recorded_at: started_at + 1_000,
        usage: RequestUsage {
            segment_id: "codex:turn-1:1".into(),
            parent: None,
            model: None,
            boundary: UsageBoundary::Request,
            input_tokens: 10,
            output_tokens: 20,
            cache_read_tokens: 30,
            cache_write_tokens: 40,
            cache_write_1h_tokens: None,
            reasoning_tokens: None,
            cost_usd_reported: Some(0.25),
            complete: true,
        },
    });
    ledger.finish_call(CallFinish {
        call_id: "call-1".into(),
        outcome: CallOutcome::Completed,
        ended_at: started_at + 2_000,
        token_usage_available: true,
    });
    ledger
}

#[tokio::test]
async fn ledger_summaries_group_rows_and_omit_dimensions_that_were_not_queried() {
    let fixture = Fixture::new();
    let (execution, _) = worker_with_ledger(
        &fixture,
        model::creation::Creations::default(),
        Box::new(fixture.client()),
        recorded_ledger(&fixture),
    );
    let totals = json!({
        "calls":1,"failedCalls":0,"requests":1,"inputTokens":10,"outputTokens":20,
        "cacheReadTokens":30,"cacheWriteTokens":40,"reportedCostUsd":0.25,
        "costReportedRequests":1,"lowerBoundCalls":0,"missingCalls":0
    });
    for (group_by, key) in [
        (
            json!(["provider", "source"]),
            json!({"provider":"codex","source":"user"}),
        ),
        (
            json!(["day", "outcome", "model"]),
            json!({"day":"2025-11-08","outcome":"completed","model":"gpt-5"}),
        ),
    ] {
        let mut row = totals.clone();
        row["key"] = key;
        assert_eq!(
            request(
                &execution,
                "provider.usage.ledger.request",
                json!({"from":LEDGER_SINCE,"to":LEDGER_SINCE + DAY_MS,"groupBy":group_by}),
            )
            .await,
            json!({"rows":[row],"since":LEDGER_SINCE})
        );
    }
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn ledger_queries_need_a_bounded_range_and_distinct_known_groups() {
    let fixture = Fixture::new();
    let (execution, _) = worker_with_ledger(
        &fixture,
        model::creation::Creations::default(),
        Box::new(fixture.client()),
        recorded_ledger(&fixture),
    );
    let longest = request(
        &execution,
        "provider.usage.ledger.request",
        json!({"from":0,"to":400 * DAY_MS,"groupBy":["provider"]}),
    )
    .await;
    assert_eq!(longest["since"], LEDGER_SINCE);
    for params in [
        json!({"from":5,"to":5,"groupBy":["provider"]}),
        json!({"from":6,"to":5,"groupBy":["provider"]}),
        json!({"from":0,"to":400 * DAY_MS + 1,"groupBy":["provider"]}),
        json!({"from":i64::MIN,"to":i64::MAX,"groupBy":["provider"]}),
        json!({"from":0,"to":DAY_MS,"groupBy":[]}),
        json!({"from":0,"to":DAY_MS,"groupBy":["provider","model","provider"]}),
        json!({"from":0,"to":DAY_MS,"groupBy":["workspace"]}),
        json!({"from":0,"to":DAY_MS}),
        json!({"from":"0","to":DAY_MS,"groupBy":["day"]}),
        json!({"from":0.5,"to":DAY_MS,"groupBy":["day"]}),
        json!({"from":0,"to":DAY_MS,"groupBy":["day"],"unknown":true}),
    ] {
        assert_eq!(
            execution
                .execute("provider.usage.ledger.request", params.clone())
                .await,
            Err(ErrorCode::InvalidMessage),
            "{params}"
        );
    }
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn ledger_summaries_are_unsupported_on_a_host_without_a_ledger() {
    let fixture = Fixture::new();
    let (execution, _) = worker(&fixture);
    assert_eq!(
        execution
            .execute(
                "provider.usage.ledger.request",
                json!({"from":0,"to":DAY_MS,"groupBy":["provider"]}),
            )
            .await,
        Err(ErrorCode::UnsupportedCapability)
    );
    execution.shutdown().await.unwrap();
}
