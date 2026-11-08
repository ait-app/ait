use super::*;
use crate::ports::account_usage::PreparedUsage;
use crate::ports::agent_session::{AgentResumePurpose, AgentSession, AgentSessionSpec};
use domain::agent_runtime::AgentPersistenceHandle;
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Debug, Default)]
struct Client {
    identity: AtomicUsize,
    unknown_source: AtomicBool,
    preparation_error: AtomicUsize,
    preparation_delay: AtomicUsize,
    credential_revision: AtomicUsize,
    preparations: AtomicUsize,
    polls: AtomicUsize,
    delay: AtomicUsize,
}

#[async_trait::async_trait]
impl AgentClient for Client {
    fn provider(&self) -> &'static str {
        "codex"
    }
    async fn create_session(
        &self,
        _spec: &AgentSessionSpec,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        Err(AgentSessionError::Unavailable)
    }
    async fn resume_session(
        &self,
        _handle: &AgentPersistenceHandle,
        _spec: &AgentSessionSpec,
        _purpose: AgentResumePurpose,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        Err(AgentSessionError::Unavailable)
    }
    async fn is_available(&self) -> Result<bool, AgentSessionError> {
        Ok(true)
    }
    async fn usage_cache_key(&self) -> Option<UsageIdentity> {
        if self.unknown_source.load(Ordering::SeqCst) {
            return None;
        }
        Some(UsageIdentity::from_native(
            "test",
            &[
                self.identity.load(Ordering::SeqCst).to_le_bytes(),
                self.credential_revision
                    .load(Ordering::SeqCst)
                    .to_le_bytes(),
            ]
            .concat(),
        ))
    }
    async fn prepare_usage(&self) -> Result<PreparedUsage<'_>, AgentSessionError> {
        self.preparations.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_secs(
            self.preparation_delay.load(Ordering::SeqCst) as u64,
        ))
        .await;
        match self.preparation_error.load(Ordering::SeqCst) {
            1 => return Err(AgentSessionError::Failed),
            2 => return Err(AgentSessionError::Unavailable),
            _ => {}
        }
        let identity = Some(UsageIdentity::from_native(
            "native-account",
            &self.identity.load(Ordering::SeqCst).to_le_bytes(),
        ));
        let generation = self.identity.load(Ordering::SeqCst);
        Ok(PreparedUsage {
            identity,
            report: Box::pin(async move {
                self.polls.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_secs(self.delay.load(Ordering::SeqCst) as u64))
                    .await;
                Ok(
                    json!({"providerId":"codex","status":"available","windows":[],"generation":generation}),
                )
            }),
        })
    }
}

#[tokio::test(start_paused = true)]
async fn fresh_host_cache_skips_preparation_and_is_shared_across_readers() {
    let cache = UsageCache::default();
    let client = Arc::new(Client::default());
    let first = cache.host(client.clone(), false, Instant::now()).await;
    let other_reader = cache.clone();
    assert_eq!(
        other_reader
            .host(client.clone(), false, Instant::now())
            .await,
        first
    );
    assert_eq!(client.preparations.load(Ordering::SeqCst), 1);
    assert_eq!(client.polls.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn stale_cache_returns_immediately_during_one_background_refresh() {
    let cache = UsageCache::default();
    let client = Arc::new(Client::default());
    let first = cache.host(client.clone(), false, Instant::now()).await;
    tokio::time::advance(Duration::from_secs(61)).await;
    client.delay.store(5, Ordering::SeqCst);
    let started = Instant::now();
    assert_eq!(cache.host(client.clone(), false, started).await, first);
    tokio::task::yield_now().await;
    assert_eq!(cache.host(client.clone(), false, started).await, first);
    assert_eq!(started.elapsed(), Duration::ZERO);
    let refreshed = cache.host(client.clone(), true, started).await;
    assert_eq!(refreshed["status"], "available");
    assert_eq!(client.polls.load(Ordering::SeqCst), 2);
    assert_eq!(client.preparations.load(Ordering::SeqCst), 2);
}

#[tokio::test(start_paused = true)]
async fn concurrent_force_calls_share_work_and_timeout_is_cached_without_new_timestamp_for_history()
{
    let cache = UsageCache::default();
    let client = Arc::new(Client::default());
    let first = cache.host(client.clone(), false, Instant::now()).await;
    client.delay.store(100, Ordering::SeqCst);
    let started = Instant::now();
    let (a, b) = tokio::join!(
        cache.host(client.clone(), true, started),
        cache.host(client.clone(), true, started)
    );
    assert_eq!(a, b);
    assert_eq!(a["status"], "error");
    assert_eq!(a["lastSuccessful"]["fetchedAt"], first["successfulAt"]);
    assert_eq!(started.elapsed(), COLLECTION_TIMEOUT);
    assert_eq!(cache.host(client.clone(), false, Instant::now()).await, a);
    assert_eq!(client.polls.load(Ordering::SeqCst), 2);
}

#[tokio::test(start_paused = true)]
async fn credential_change_fences_old_inflight_refresh_and_never_reuses_other_account() {
    let cache = UsageCache::default();
    let client = Arc::new(Client::default());
    cache.host(client.clone(), false, Instant::now()).await;
    tokio::time::advance(Duration::from_secs(61)).await;
    client.delay.store(10, Ordering::SeqCst);
    cache.host(client.clone(), false, Instant::now()).await;
    tokio::task::yield_now().await;
    client.identity.store(1, Ordering::SeqCst);
    client.delay.store(0, Ordering::SeqCst);
    let new = cache.host(client.clone(), false, Instant::now()).await;
    assert_eq!(new["generation"], 1);
    assert!(new["lastSuccessful"].is_null());
    tokio::time::advance(Duration::from_secs(11)).await;
    tokio::task::yield_now().await;
    assert_eq!(
        cache.host(client.clone(), false, Instant::now()).await["generation"],
        1
    );
}

#[tokio::test]
async fn disconnected_cold_reader_does_not_cancel_shared_refresh() {
    let cache = UsageCache::default();
    let client = Arc::new(Client::default());
    client.delay.store(1, Ordering::SeqCst);
    let pending = {
        let cache = cache.clone();
        let client = client.clone();
        tokio::spawn(async move { cache.host(client, false, Instant::now()).await })
    };
    while client.polls.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
    }
    pending.abort();
    let value = cache.host(client.clone(), false, Instant::now()).await;
    assert_eq!(value["status"], "available");
    assert_eq!(client.polls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
#[ignore = "requires an installed Codex and native login; prints timings only"]
async fn installed_codex_host_cache_timing() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_test_writer()
        .try_init();
    let cache = UsageCache::default();
    let client = Arc::new(crate::local::codex::CodexClient::new(
        "/usr/bin/codex".into(),
    ));
    assert!(
        client.usage_cache_key().await.is_some(),
        "this audit requires file-backed credentials"
    );
    for (name, force) in [
        ("cold", false),
        ("cached", false),
        ("refresh", true),
        ("cached-after-refresh", false),
    ] {
        let started = Instant::now();
        let report = cache.host(client.clone(), force, started).await;
        tracing::info!(
            phase = name,
            elapsed_ms = started.elapsed().as_millis(),
            status = report["status"].as_str(),
            "quota cache timing"
        );
        assert_eq!(report["status"], "available");
        if name.starts_with("cached") {
            assert!(started.elapsed() < Duration::from_secs(1));
        }
    }
}

#[tokio::test(start_paused = true)]
async fn changed_credentials_with_same_native_identity_refresh_once_then_restore_fast_cache() {
    let cache = UsageCache::default();
    let client = Arc::new(Client::default());
    cache.host(client.clone(), false, Instant::now()).await;
    client.credential_revision.store(1, Ordering::SeqCst);
    let refreshed = cache.host(client.clone(), false, Instant::now()).await;
    assert_eq!(refreshed["status"], "available");
    assert_eq!(
        cache.host(client.clone(), false, Instant::now()).await,
        refreshed
    );
    assert_eq!(client.preparations.load(Ordering::SeqCst), 2);
    assert_eq!(client.polls.load(Ordering::SeqCst), 2);
}

#[tokio::test(start_paused = true)]
async fn changed_credentials_with_matching_native_identity_do_not_inherit_failure_history() {
    let cache = UsageCache::default();
    let client = Arc::new(Client::default());
    cache.host(client.clone(), false, Instant::now()).await;
    client.credential_revision.store(1, Ordering::SeqCst);
    client.delay.store(100, Ordering::SeqCst);
    let failed = cache.host(client.clone(), false, Instant::now()).await;
    assert_eq!(failed["status"], "error");
    assert!(failed["lastSuccessful"].is_null());
}

#[tokio::test(start_paused = true)]
async fn unknown_sources_verify_native_identity_instead_of_joining_another_account() {
    let cache = UsageCache::default();
    let client = Arc::new(Client::default());
    client.unknown_source.store(true, Ordering::SeqCst);
    client.delay.store(5, Ordering::SeqCst);
    let first = {
        let cache = cache.clone();
        let client = client.clone();
        tokio::spawn(async move { cache.host(client, false, Instant::now()).await })
    };
    while client.polls.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
    }
    client.identity.store(1, Ordering::SeqCst);
    client.delay.store(0, Ordering::SeqCst);
    let second = cache.host(client.clone(), false, Instant::now()).await;
    assert_eq!(second["generation"], 1);
    assert!(second["lastSuccessful"].is_null());
    assert_eq!(client.preparations.load(Ordering::SeqCst), 2);
    assert_eq!(client.polls.load(Ordering::SeqCst), 2);
    assert_ne!(first.await.unwrap()["generation"], 1);
}

#[tokio::test(start_paused = true)]
async fn switching_to_unknown_source_repolls_and_clears_history_despite_matching_native_identity() {
    for delay in [0, 100] {
        let cache = UsageCache::default();
        let client = Arc::new(Client::default());
        cache.host(client.clone(), false, Instant::now()).await;
        client.unknown_source.store(true, Ordering::SeqCst);
        client.delay.store(delay, Ordering::SeqCst);
        let report = cache.host(client.clone(), false, Instant::now()).await;
        assert_eq!(
            report["status"],
            if delay == 0 { "available" } else { "error" }
        );
        assert!(report["lastSuccessful"].is_null());
        assert_eq!(client.polls.load(Ordering::SeqCst), 2);
    }
}

#[tokio::test(start_paused = true)]
async fn verified_preparation_failures_and_timeouts_are_cached_for_fifteen_seconds() {
    for delay in [0, 100] {
        let cache = UsageCache::default();
        let client = Arc::new(Client::default());
        client.preparation_error.store(1, Ordering::SeqCst);
        client.preparation_delay.store(delay, Ordering::SeqCst);
        let failed = cache.host(client.clone(), false, Instant::now()).await;
        assert_eq!(failed["status"], "error");
        assert!(failed["lastSuccessful"].is_null());
        let started = Instant::now();
        assert_eq!(cache.host(client.clone(), false, started).await, failed);
        assert_eq!(started.elapsed(), Duration::ZERO);
        assert_eq!(client.preparations.load(Ordering::SeqCst), 1);
        tokio::time::advance(Duration::from_secs(15)).await;
        client.preparation_error.store(0, Ordering::SeqCst);
        client.preparation_delay.store(0, Ordering::SeqCst);
        assert_eq!(
            cache.host(client.clone(), false, Instant::now()).await["status"],
            "available"
        );
        assert_eq!(client.preparations.load(Ordering::SeqCst), 2);
    }
}

#[tokio::test(start_paused = true)]
async fn preparation_unavailable_remains_filterable_and_unknown_failures_are_not_cached() {
    let cache = UsageCache::default();
    let client = Arc::new(Client::default());
    client.preparation_error.store(2, Ordering::SeqCst);
    assert_eq!(
        cache.host(client.clone(), false, Instant::now()).await["status"],
        "unavailable"
    );
    client.unknown_source.store(true, Ordering::SeqCst);
    client.preparation_error.store(1, Ordering::SeqCst);
    for _ in 0..2 {
        assert_eq!(
            cache.host(client.clone(), false, Instant::now()).await["status"],
            "error"
        );
    }
    assert_eq!(client.preparations.load(Ordering::SeqCst), 3);
}

#[tokio::test(start_paused = true)]
async fn changed_source_preparation_failure_does_not_inherit_previous_success() {
    let cache = UsageCache::default();
    let client = Arc::new(Client::default());
    cache.host(client.clone(), false, Instant::now()).await;
    client.credential_revision.store(1, Ordering::SeqCst);
    client.preparation_error.store(1, Ordering::SeqCst);
    let failed = cache.host(client.clone(), false, Instant::now()).await;
    assert_eq!(failed["status"], "error");
    assert!(failed["lastSuccessful"].is_null());
    assert_eq!(
        cache.host(client.clone(), false, Instant::now()).await,
        failed
    );
    assert_eq!(client.preparations.load(Ordering::SeqCst), 2);
}

#[tokio::test(start_paused = true)]
async fn shutdown_drains_detached_refreshes_and_rejects_new_work() {
    let cache = UsageCache::default();
    let client = Arc::new(Client::default());
    cache.host(client.clone(), false, Instant::now()).await;
    tokio::time::advance(Duration::from_secs(61)).await;
    client.delay.store(5, Ordering::SeqCst);
    cache.host(client.clone(), false, Instant::now()).await;
    while client.polls.load(Ordering::SeqCst) < 2 {
        tokio::task::yield_now().await;
    }
    let started = Instant::now();
    cache.close().await.unwrap();
    assert_eq!(started.elapsed(), Duration::from_secs(5));
    assert_eq!(
        cache.host(client.clone(), true, Instant::now()).await["status"],
        "error"
    );
    assert_eq!(client.polls.load(Ordering::SeqCst), 2);
}
