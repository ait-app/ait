use super::*;
use serde_json::json;

fn record(usage: &Value) -> PersistedAgentRuntimeRecord {
    serde_json::from_value(json!({
        "id":"selected", "provider":"codex", "cwd":"/tmp",
        "createdAt":"2026-11-11T00:00:00Z", "updatedAt":"2026-11-11T00:00:00Z",
        "lastStatus":"running", "config":{"model":"requested"},
        "runtimeInfo":{"provider":"codex","sessionId":"native",
            "model":"actual", "extra":{"lastUsage":usage}}
    }))
    .unwrap()
}

#[test]
fn statistics_preserve_null_zero_models_and_provider_scope() {
    let source = record(&json!({"inputTokens":0,"totalCostUsd":0.0}));
    let result = serde_json::to_value(project(&source)).unwrap();
    assert_eq!(result["configuredModel"], "requested");
    assert_eq!(result["effectiveModel"], "actual");
    assert_eq!(result["status"], "running");
    assert_eq!(result["usageScope"], "provider_snapshot");
    assert_eq!(result["usage"]["inputTokens"], 0);
    assert_eq!(result["usage"]["totalCostUsd"], 0.0);
    assert_eq!(result["usage"]["outputTokens"], Value::Null);
    assert_eq!(result["usage"]["cachedInputTokens"], Value::Null);
    assert_eq!(result["usage"]["contextWindowMaxTokens"], Value::Null);
    assert_eq!(result["usage"]["contextWindowUsedTokens"], Value::Null);
}

#[test]
fn statistics_replace_cumulative_snapshots_and_reject_malformed_facts() {
    for total in [100, 100, 150] {
        let result = serde_json::to_value(project(&record(&json!({"inputTokens":total})))).unwrap();
        assert_eq!(result["usage"]["inputTokens"], total);
    }
    for usage in [
        json!({"totalCostUsd":-1}),
        json!({"inputTokens":"100"}),
        json!({"tokens":100}),
    ] {
        let result = serde_json::to_value(project(&record(&usage))).unwrap();
        assert!(
            result["usage"]
                .as_object()
                .unwrap()
                .values()
                .all(Value::is_null)
        );
    }
    let mut source = record(&json!({}));
    source.runtime_info = None;
    let result = serde_json::to_value(project(&source)).unwrap();
    assert_eq!(result["effectiveModel"], Value::Null);
    assert_eq!(result["configuredModel"], "requested");
}
