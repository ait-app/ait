use std::sync::atomic::{AtomicUsize, Ordering};

use crate::ports::account_usage::{PreparedUsage, UsageIdentity};

use super::*;

fn identity(account: &str) -> UsageIdentity {
    UsageIdentity::from_native("codex", account.as_bytes())
}

fn available() -> Value {
    json!({"providerId":"codex","accountLabel":"account@example.test","status":"available",
        "planLabel":"Pro","windows":[{"id":"weekly","usedPercent":42}],
        "balances":[{"id":"credits","remaining":2}],"details":[{"id":"tier","value":"paid"}]})
}

#[test]
fn quota_cache_reuses_bounded_facts_and_force_refresh_bypasses_them() {
    let cache = UsageCache::default();
    let account = identity("account");
    let value = cache.put("codex", Some(account.clone()), available(), "attempt");
    assert_eq!(cache.get("codex", &account, false), Some(value));
    assert!(cache.get("codex", &account, true).is_none());
    assert!(cache.get("other", &account, false).is_none());
    assert!(cache.get("codex", &identity("different"), false).is_none());
    cache.0.lock().unwrap().get_mut("codex").unwrap().fetched = Instant::now()
        .checked_sub(Duration::from_secs(301))
        .unwrap();
    assert!(cache.get("codex", &account, false).is_none());
    for index in 0..65 {
        cache.put(
            &format!("provider-{index}"),
            Some(account.clone()),
            available(),
            "attempt",
        );
    }
    assert!(cache.0.lock().unwrap().len() <= 64);
    assert!(cache.get("provider-64", &account, false).is_some());
}

#[test]
fn failures_retain_success_without_promoting_attempt_time_and_recovery_replaces_it() {
    let cache = UsageCache::default();
    let account = identity("account");
    let first = cache.put("codex", Some(account.clone()), available(), "first");
    let failed = cache.put(
        "codex",
        Some(account.clone()),
        failure("codex", AgentSessionError::Failed),
        "second",
    );
    assert_eq!(failed["status"], "error");
    assert_eq!(failed["fetchedAt"], "second");
    assert!(failed["successfulAt"].is_null());
    assert_eq!(failed["lastSuccessful"]["fetchedAt"], first["successfulAt"]);
    assert_eq!(
        failed["lastSuccessful"]["report"]["windows"],
        first["windows"]
    );
    assert_eq!(failed["accountLabel"], first["accountLabel"]);
    let unavailable = cache.put(
        "codex",
        Some(account.clone()),
        failure("codex", AgentSessionError::Unavailable),
        "third",
    );
    assert_eq!(unavailable["status"], "unavailable");
    assert_eq!(unavailable["problem"]["kind"], "no_quota");
    assert_eq!(unavailable["lastSuccessful"], failed["lastSuccessful"]);
    let mut recovered = available();
    recovered["windows"][0]["usedPercent"] = json!(60);
    let recovered = cache.put("codex", Some(account), recovered, "fourth");
    assert!(recovered["lastSuccessful"].is_null());
    assert_eq!(recovered["windows"][0]["usedPercent"], 60);
}

#[test]
fn account_changes_unknown_identity_and_independent_lifetimes_clear_history() {
    let host = UsageCache::default();
    host.put("codex", Some(identity("first")), available(), "first");
    let other = host.put(
        "codex",
        Some(identity("second")),
        failure("codex", AgentSessionError::Failed),
        "next",
    );
    assert!(other["lastSuccessful"].is_null());
    host.put("codex", Some(identity("second")), available(), "success");
    let unknown = host.put(
        "codex",
        None,
        failure("codex", AgentSessionError::Failed),
        "unknown",
    );
    assert!(unknown["lastSuccessful"].is_null());
    assert!(host.0.lock().unwrap().is_empty());
    host.put("codex", Some(identity("first")), available(), "host");
    let session = UsageCache::default();
    let resumed = UsageCache::default();
    for cache in [&session, &resumed] {
        let value = cache.put(
            "codex",
            Some(identity("first")),
            failure("codex", AgentSessionError::Failed),
            "session",
        );
        assert!(value["lastSuccessful"].is_null());
    }
}

#[tokio::test]
async fn cache_checks_identity_before_polling_and_force_preserves_failure_history() {
    let cache = UsageCache::default();
    let calls = AtomicUsize::new(0);
    let poll = || PreparedUsage {
        identity: Some(identity("account")),
        report: Box::pin(async {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(available())
        }),
    };
    let first = cache.read("codex", Ok(poll()), false).await;
    let cached = cache.read("codex", Ok(poll()), false).await;
    assert_eq!(first, cached);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    let failed = cache
        .read(
            "codex",
            Ok(PreparedUsage {
                identity: Some(identity("account")),
                report: Box::pin(async { Err(AgentSessionError::Failed) }),
            }),
            true,
        )
        .await;
    assert_eq!(failed["status"], "error");
    assert_eq!(failed["lastSuccessful"]["fetchedAt"], first["successfulAt"]);
    let unresolved = cache
        .read("codex", Err(AgentSessionError::Failed), true)
        .await;
    assert!(unresolved["lastSuccessful"].is_null());
    assert!(cache.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn unknown_provider_is_rejected_without_querying_other_accounts() {
    let manager = AgentManager::new(Box::new(super::super::tests::MemoryRegistry::default()));
    assert_eq!(
        manager.usage(None, Some("unknown"), false).await,
        Err(ErrorCode::UnsupportedCapability)
    );
}
