use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::*;

#[derive(Debug)]
struct Gate {
    calls: AtomicUsize,
    entered: Semaphore,
    release: Semaphore,
    failed: AtomicBool,
}

impl Gate {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
            failed: AtomicBool::new(false),
        })
    }

    async fn entered(&self, count: u32) {
        tokio::time::timeout(Duration::from_secs(5), self.entered.acquire_many(count))
            .await
            .unwrap()
            .unwrap()
            .forget();
    }
}

#[derive(Debug)]
struct ParallelClient {
    provider: &'static str,
    gate: Arc<Gate>,
}

#[async_trait::async_trait]
impl AgentClient for ParallelClient {
    fn discovery_cache_ttl(&self) -> Duration {
        if self.provider == "omp" {
            Duration::MAX
        } else {
            Duration::from_mins(5)
        }
    }

    fn discovery_timeout(&self) -> Duration {
        if self.provider == "opencode" {
            Duration::from_secs(300)
        } else {
            Duration::from_secs(30)
        }
    }

    fn provider(&self) -> &'static str {
        self.provider
    }
    async fn is_available(&self) -> Result<bool, AgentSessionError> {
        Ok(true)
    }
    async fn discover(&self, _cwd: &str) -> Result<Details, AgentSessionError> {
        self.gate.calls.fetch_add(1, Ordering::SeqCst);
        self.gate.entered.add_permits(1);
        self.gate.release.acquire().await.unwrap().forget();
        if self.gate.failed.load(Ordering::SeqCst) {
            return Err(AgentSessionError::Failed);
        }
        Ok(Details {
            models: vec![json!({"id":"test","provider":self.provider})],
            modes: vec![],
            features: vec![],
        })
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
}

#[tokio::test]
async fn provider_specific_budget_keeps_a_cold_catalog_loading() {
    let root = tempfile::tempdir().unwrap();
    let gate = Gate::new();
    let clients = clients(&[("opencode", gate.clone())]);
    let catalog = Catalog::default();
    let events = SessionEvents::default();
    catalog
        .read(
            &clients,
            &events,
            "provider.snapshot.get.request",
            json!({"cwd":root.path()}),
        )
        .await
        .unwrap();
    gate.entered(1).await;

    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(31)).await;
    let snapshot = catalog
        .read(
            &clients,
            &events,
            "provider.snapshot.get.request",
            json!({"cwd":root.path()}),
        )
        .await
        .unwrap();
    assert_eq!(snapshot["entries"][0]["status"], "loading");
    tokio::time::resume();

    gate.release.add_permits(1);
    let ready = catalog
        .execute(
            &clients,
            &events,
            "provider.models.list.request",
            json!({"provider":"opencode","cwd":root.path()}),
        )
        .await
        .unwrap();
    assert_eq!(ready["models"][0]["provider"], "opencode");
    catalog.close().await;
}

fn clients(probes: &[(&'static str, Arc<Gate>)]) -> BTreeMap<String, Arc<dyn AgentClient>> {
    probes
        .iter()
        .map(|(provider, gate)| {
            (
                provider.to_string(),
                Arc::new(ParallelClient {
                    provider,
                    gate: gate.clone(),
                }) as Arc<dyn AgentClient>,
            )
        })
        .collect()
}

#[tokio::test]
async fn background_snapshot_returns_loading_and_other_provider_commits_while_one_is_blocked() {
    let root = tempfile::tempdir().unwrap();
    let slow = Gate::new();
    let fast = Gate::new();
    fast.release.add_permits(1);
    let clients = clients(&[("codex", slow.clone()), ("claude", fast.clone())]);
    let catalog = Catalog::default();
    let events = SessionEvents::default();
    let first = catalog
        .read(
            &clients,
            &events,
            "provider.snapshot.get.request",
            json!({"cwd":root.path()}),
        )
        .await
        .unwrap();
    assert!(
        first["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["status"] == "loading")
    );
    slow.entered(1).await;
    let ready = tokio::time::timeout(
        Duration::from_secs(5),
        catalog.read(
            &clients,
            &events,
            "provider.models.list.request",
            json!({"provider":"claude","cwd":root.path()}),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(ready["models"][0]["provider"], "claude");
    let partial = catalog
        .read(
            &clients,
            &events,
            "provider.snapshot.get.request",
            json!({"cwd":root.path()}),
        )
        .await
        .unwrap();
    assert_eq!(partial["refreshing"], json!(["codex"]));
    assert!(partial["revision"].as_u64().unwrap() > first["revision"].as_u64().unwrap());
    assert_eq!(slow.calls.load(Ordering::SeqCst), 1);
    catalog.close().await;
}

#[tokio::test]
async fn repeated_reads_share_a_probe_and_refreshes_coalesce_to_one_followup() {
    let root = tempfile::tempdir().unwrap();
    let gate = Gate::new();
    let clients = clients(&[("codex", gate.clone())]);
    let catalog = Catalog::default();
    let events = SessionEvents::default();
    catalog
        .read(
            &clients,
            &events,
            "provider.snapshot.get.request",
            json!({"cwd":root.path()}),
        )
        .await
        .unwrap();
    gate.entered(1).await;
    for _ in 0..20 {
        catalog
            .read(
                &clients,
                &events,
                "provider.snapshot.get.request",
                json!({"cwd":root.path()}),
            )
            .await
            .unwrap();
        catalog
            .read(
                &clients,
                &events,
                "provider.snapshot.refresh.request",
                json!({"cwd":root.path(),"providers":["codex"]}),
            )
            .await
            .unwrap();
    }
    assert_eq!(gate.calls.load(Ordering::SeqCst), 1);
    gate.release.add_permits(1);
    gate.entered(1).await;
    assert_eq!(gate.calls.load(Ordering::SeqCst), 2);
    gate.release.add_permits(1);
    let ready = catalog
        .execute(
            &clients,
            &events,
            "provider.snapshot.get.request",
            json!({"cwd":root.path()}),
        )
        .await
        .unwrap();
    assert_eq!(ready["refreshing"], json!([]));
    assert_eq!(gate.calls.load(Ordering::SeqCst), 2);
    catalog.close().await;
}

#[tokio::test]
async fn one_provider_has_at_most_four_concurrent_scopes() {
    let roots = (0..5)
        .map(|_| tempfile::tempdir().unwrap())
        .collect::<Vec<_>>();
    let gate = Gate::new();
    let clients = clients(&[("codex", gate.clone())]);
    let catalog = Catalog::default();
    let events = SessionEvents::default();
    for root in &roots {
        catalog
            .read(
                &clients,
                &events,
                "provider.snapshot.get.request",
                json!({"cwd":root.path()}),
            )
            .await
            .unwrap();
    }
    gate.entered(4).await;
    assert_eq!(gate.calls.load(Ordering::SeqCst), 4);
    gate.release.add_permits(5);
    for root in &roots {
        let ready = catalog
            .execute(
                &clients,
                &events,
                "provider.snapshot.get.request",
                json!({"cwd":root.path()}),
            )
            .await
            .unwrap();
        assert_eq!(ready["entries"][0]["status"], "ready");
    }
    assert_eq!(gate.calls.load(Ordering::SeqCst), 5);
    catalog.close().await;
}

#[tokio::test]
async fn access_driven_catalog_keeps_old_models_until_explicit_background_refresh() {
    let root = tempfile::tempdir().unwrap();
    let gate = Gate::new();
    gate.release.add_permits(1);
    let clients = clients(&[("omp", gate.clone())]);
    let catalog = Catalog::default();
    let events = SessionEvents::default();
    let params = json!({"cwd":root.path()});
    let method = "provider.snapshot.get.request";
    let original = catalog
        .execute(&clients, &events, method, params.clone())
        .await
        .unwrap();
    gate.entered(1).await;
    expire_models(&catalog, root.path(), "omp", Duration::from_hours(24));
    let retained = catalog
        .read(&clients, &events, method, params.clone())
        .await
        .unwrap();
    assert_eq!(retained["entries"], original["entries"]);
    assert_eq!(retained["refreshing"], json!([]));
    assert_eq!(gate.calls.load(Ordering::SeqCst), 1);

    let refreshing = catalog
        .read(
            &clients,
            &events,
            "provider.snapshot.refresh.request",
            json!({"cwd":root.path(),"providers":["omp"]}),
        )
        .await
        .unwrap();
    assert_eq!(refreshing["acknowledged"], true);
    gate.entered(1).await;
    let pending = catalog
        .read(&clients, &events, method, params.clone())
        .await
        .unwrap();
    assert_eq!(pending["entries"], original["entries"]);
    assert_eq!(pending["refreshing"], json!(["omp"]));
    gate.failed.store(true, Ordering::SeqCst);
    gate.release.add_permits(1);
    let failed = catalog
        .execute(&clients, &events, method, params)
        .await
        .unwrap();
    assert_eq!(failed["entries"][0]["status"], "ready");
    assert_eq!(
        failed["entries"][0]["models"],
        original["entries"][0]["models"]
    );
    assert_eq!(
        failed["entries"][0]["fetchedAt"],
        original["entries"][0]["fetchedAt"]
    );
    assert_eq!(failed["entries"][0]["error"], "Provider discovery failed");
    catalog.close().await;
}

#[tokio::test]
async fn expired_models_refresh_once_in_background_and_survive_a_failed_refresh() {
    let root = tempfile::tempdir().unwrap();
    let gate = Gate::new();
    gate.release.add_permits(1);
    let clients = clients(&[("codex", gate.clone())]);
    let catalog = Catalog::default();
    let events = SessionEvents::default();
    let params = json!({"cwd":root.path()});
    let method = "provider.snapshot.get.request";
    let original = catalog
        .execute(&clients, &events, method, params.clone())
        .await
        .unwrap();
    gate.entered(1).await;
    for _ in 0..5 {
        catalog
            .read(&clients, &events, method, params.clone())
            .await
            .unwrap();
    }
    assert_eq!(gate.calls.load(Ordering::SeqCst), 1);

    expire_models(&catalog, root.path(), "codex", Duration::from_secs(301));
    let refreshing = catalog
        .read(&clients, &events, method, params.clone())
        .await
        .unwrap();
    assert_eq!(
        refreshing["entries"][0]["models"],
        original["entries"][0]["models"]
    );
    assert_eq!(refreshing["refreshing"], json!(["codex"]));
    gate.entered(1).await;
    for _ in 0..5 {
        catalog
            .read(&clients, &events, method, params.clone())
            .await
            .unwrap();
    }
    assert_eq!(gate.calls.load(Ordering::SeqCst), 2);
    gate.failed.store(true, Ordering::SeqCst);
    gate.release.add_permits(1);
    let failed = catalog
        .execute(&clients, &events, method, params.clone())
        .await
        .unwrap();
    assert_eq!(failed["entries"][0]["status"], "ready");
    assert_eq!(
        failed["entries"][0]["models"],
        original["entries"][0]["models"]
    );
    assert_eq!(
        failed["entries"][0]["fetchedAt"],
        original["entries"][0]["fetchedAt"]
    );
    assert_eq!(failed["entries"][0]["error"], "Provider discovery failed");
    catalog
        .read(&clients, &events, method, params.clone())
        .await
        .unwrap();
    assert_eq!(gate.calls.load(Ordering::SeqCst), 2);

    expire_models(&catalog, root.path(), "codex", Duration::from_secs(31));
    catalog
        .read(&clients, &events, method, params.clone())
        .await
        .unwrap();
    gate.entered(1).await;
    gate.failed.store(false, Ordering::SeqCst);
    gate.release.add_permits(1);
    let recovered = catalog
        .execute(&clients, &events, method, params)
        .await
        .unwrap();
    assert_eq!(recovered["entries"][0]["status"], "ready");
    assert!(recovered["entries"][0]["error"].is_null());
    assert_eq!(gate.calls.load(Ordering::SeqCst), 3);
    catalog.close().await;
}

fn expire_models(catalog: &Catalog, cwd: &std::path::Path, provider: &str, age: Duration) {
    catalog
        .cache
        .lock()
        .unwrap()
        .snapshots
        .get_mut(&Some(
            cwd.canonicalize().unwrap().to_str().unwrap().to_owned(),
        ))
        .unwrap()
        .entries
        .get_mut(provider)
        .unwrap()
        .fetched = Some(Instant::now().checked_sub(age).unwrap());
}
