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

#[test]
fn usage_boundary_uses_snake_case_names_for_storage_and_the_wire() {
    let variants = [
        UsageBoundary::Request,
        UsageBoundary::Turn,
        UsageBoundary::CumulativeDelta,
    ];
    let expected = ["request", "turn", "cumulative_delta"];

    assert_eq!(variants.map(UsageBoundary::as_str), expected);
    assert_eq!(variants.map(|variant| serde_json::json!(variant)), expected);
}
