use super::*;

#[test]
fn usage_preserves_buckets_reports_missing_windows_and_rejects_malformed_metrics() {
    let result = usage(&json!({"rateLimitsByLimitId":{"codex":{"planType":"plus","primary":{"usedPercent":105,"resetsAt":1_700_000_000}},"review":{"secondary":{"usedPercent":80}}}})).unwrap();
    assert_eq!(result["windows"][0]["remainingPct"], 0);
    assert_eq!(result["windows"][0]["tone"], "danger");
    assert_eq!(result["windows"][1]["tone"], "warning");
    assert_eq!(result["planLabel"], "plus");
    assert!(result["windows"][0].get("shortLabel").is_none());
    assert_eq!(
        usage(&json!({"rateLimits":{"primary":{"usedPercent":25,"windowDurationMins":300},"secondary":{"usedPercent":30,"windowDurationMins":10080}}})).unwrap()["windows"][0]["shortLabel"],
        "5h"
    );
    assert_eq!(
        usage(&json!({"rateLimits":{}})).unwrap()["status"],
        "unavailable"
    );
    for response in [
        json!({}),
        json!({"rateLimits":{"primary":{"usedPercent":-1}}}),
        json!({"rateLimits":{"primary":{"usedPercent":1,"resetsAt":i64::MAX}}}),
    ] {
        assert!(usage(&response).is_err());
    }
}

#[cfg(unix)]
#[tokio::test]
async fn diagnostics_do_not_expose_private_account_fields() {
    let fixture = crate::test_support::Fixture::new();
    std::fs::write(
        fixture.program.with_extension("cwd"),
        fixture.cwd.to_str().unwrap(),
    )
    .unwrap();
    let diagnostic = fixture.client().diagnostic().await.unwrap();
    assert!(diagnostic.contains("ChatGPT login"));
    assert!(!diagnostic.contains("private@example.test"));
    let result = fixture.client().usage().await.unwrap();
    assert_eq!(result["status"], "available");
    assert_eq!(result["windows"][0]["usedPct"], 25);
    let requests = fixture.requests();
    for method in ["account/read", "account/rateLimits/read"] {
        assert!(requests.iter().any(|request| request["method"] == method));
    }
    let client = CodexClient::new(fixture.root.path().join("missing"));
    assert_eq!(
        client.diagnostic().await.unwrap(),
        "Codex executable: unavailable"
    );
    assert!(client.usage().await.is_err());
}
