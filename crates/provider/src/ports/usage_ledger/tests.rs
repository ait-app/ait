use std::sync::Arc;

use super::*;

const MAX: u64 = 9_007_199_254_740_991;

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

#[test]
fn request_usage_accepts_exact_counters_and_subsets_within_their_bucket() {
    assert!(request().is_valid());
    assert!(
        RequestUsage {
            input_tokens: MAX,
            output_tokens: MAX,
            cache_read_tokens: MAX,
            cache_write_tokens: MAX,
            cache_write_1h_tokens: None,
            reasoning_tokens: None,
            cost_usd_reported: None,
            ..request()
        }
        .is_valid()
    );
}

#[test]
fn request_usage_rejects_counters_beyond_exact_json_integers() {
    let invalid = [
        RequestUsage {
            input_tokens: MAX + 1,
            ..request()
        },
        RequestUsage {
            output_tokens: MAX + 1,
            reasoning_tokens: None,
            ..request()
        },
        RequestUsage {
            cache_read_tokens: MAX + 1,
            ..request()
        },
        RequestUsage {
            cache_write_tokens: MAX + 1,
            cache_write_1h_tokens: None,
            ..request()
        },
    ];

    assert!(invalid.iter().all(|usage| !usage.is_valid()));
}

#[test]
fn request_usage_rejects_subsets_larger_than_their_parent_bucket() {
    let invalid = [
        RequestUsage {
            cache_write_1h_tokens: Some(41),
            ..request()
        },
        RequestUsage {
            reasoning_tokens: Some(21),
            ..request()
        },
    ];

    assert!(invalid.iter().all(|usage| !usage.is_valid()));
}

#[test]
fn request_usage_rejects_unusable_costs_and_segment_identity() {
    let invalid = [
        RequestUsage {
            cost_usd_reported: Some(-0.01),
            ..request()
        },
        RequestUsage {
            cost_usd_reported: Some(f64::NAN),
            ..request()
        },
        RequestUsage {
            cost_usd_reported: Some(f64::INFINITY),
            ..request()
        },
        RequestUsage {
            segment_id: String::new(),
            ..request()
        },
    ];

    assert!(invalid.iter().all(|usage| !usage.is_valid()));
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
            UsageBoundary::Request,
            UsageBoundary::Turn,
            UsageBoundary::CumulativeDelta,
        ],
        UsageBoundary::as_str,
        &["request", "turn", "cumulative_delta"],
    );
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
