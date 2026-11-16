use std::sync::Arc;

use provider_core::ports::usage_ledger::{RequestUsage, UsageBoundary};

use super::*;

fn request() -> RequestUsage {
    RequestUsage {
        segment_id: "claude:turn-1:msg_1".into(),
        parent: None,
        model: Some("claude-sonnet".into()),
        boundary: UsageBoundary::Request,
        input_tokens: 10,
        output_tokens: 20,
        cache_read_tokens: 30,
        cache_write_tokens: 40,
        cache_write_1h_tokens: Some(40),
        reasoning_tokens: Some(20),
        cost_usd_reported: Some(0.0),
        complete: true,
    }
}

/// Assert that each variant's storage name equals its wire name and the expected text.
fn assert_names<T: Serialize + Copy>(
    variants: &[T],
    name: fn(T) -> &'static str,
    expected: &[&str],
) {
    let stored: Vec<_> = variants.iter().copied().map(name).collect();
    let wire: Vec<_> = variants
        .iter()
        .map(|variant| serde_json::json!(variant))
        .collect();

    assert_eq!(stored, expected);
    assert_eq!(wire, expected);
}

#[test]
fn enums_use_snake_case_names_for_storage_and_the_wire() {
    assert_names(
        &[
            CallSource::User,
            CallSource::Bot,
            CallSource::Schedule,
            CallSource::Heartbeat,
            CallSource::Autonomous,
            CallSource::Summary,
        ],
        CallSource::as_str,
        &[
            "user",
            "bot",
            "schedule",
            "heartbeat",
            "autonomous",
            "summary",
        ],
    );
    assert_names(
        &[
            CallOutcome::Running,
            CallOutcome::Completed,
            CallOutcome::Failed,
            CallOutcome::Cancelled,
            CallOutcome::Rejected,
            CallOutcome::Interrupted,
        ],
        CallOutcome::as_str,
        &[
            "running",
            "completed",
            "failed",
            "cancelled",
            "rejected",
            "interrupted",
        ],
    );
    assert_names(
        &[
            UsageStatus::Exact,
            UsageStatus::LowerBound,
            UsageStatus::Missing,
        ],
        UsageStatus::as_str,
        &["exact", "lower_bound", "missing"],
    );
    assert_eq!(
        serde_json::from_value::<Vec<UsageGroup>>(serde_json::json!([
            "provider", "model", "source", "outcome", "day"
        ]))
        .unwrap(),
        [
            UsageGroup::Provider,
            UsageGroup::Model,
            UsageGroup::Source,
            UsageGroup::Outcome,
            UsageGroup::Day,
        ]
    );
}

#[tokio::test]
async fn noop_ledger_discards_writes_and_cannot_summarize() {
    let ledger: Arc<dyn UsageLedger> = Arc::new(NoopUsageLedger);

    ledger.recover_interrupted(1);
    ledger.begin_call(CallBegin {
        call_id: "call-1".into(),
        agent_id: "agent-1".into(),
        provider: "claude".into(),
        model: None,
        source: CallSource::User,
        workspace_id: None,
        started_at: 1,
    });
    ledger.record_request(RequestRecord {
        call_id: "call-1".into(),
        agent_id: "agent-1".into(),
        recorded_at: 2,
        usage: request(),
    });
    ledger.finish_call(CallFinish {
        call_id: "call-1".into(),
        outcome: CallOutcome::Completed,
        ended_at: 3,
        token_usage_available: true,
    });

    assert_eq!(
        ledger
            .summarize(UsageQuery {
                from: 0,
                to: 4,
                group_by: vec![UsageGroup::Provider],
            })
            .await,
        Err(UsageLedgerError::Unavailable)
    );
}
