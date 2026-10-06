use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

#[derive(Debug)]
struct Gate {
    calls: AtomicUsize,
    entered: Semaphore,
    release: Semaphore,
}

impl Gate {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
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

impl AgentClient for ParallelClient {
    fn provider(&self) -> &'static str {
        self.provider
    }
    fn is_available(&self) -> AgentSessionFuture<'_, bool> {
        Box::pin(async { Ok(true) })
    }
    fn discover<'a>(&'a self, _cwd: &'a str) -> AgentSessionFuture<'a, Details> {
        Box::pin(async move {
            self.gate.calls.fetch_add(1, Ordering::SeqCst);
            self.gate.entered.add_permits(1);
            self.gate.release.acquire().await.unwrap().forget();
            Ok(Details {
                models: vec![json!({"id":"test","provider":self.provider})],
                modes: vec![],
                features: vec![],
            })
        })
    }
    fn create_session<'a>(
        &'a self,
        _spec: &'a AgentSessionSpec,
    ) -> AgentSessionFuture<'a, Box<dyn AgentSession>> {
        Box::pin(async { Err(AgentSessionError::Unavailable) })
    }
    fn resume_session<'a>(
        &'a self,
        _handle: &'a AgentPersistenceHandle,
        _spec: &'a AgentSessionSpec,
        _purpose: AgentResumePurpose,
    ) -> AgentSessionFuture<'a, Box<dyn AgentSession>> {
        Box::pin(async { Err(AgentSessionError::Unavailable) })
    }
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
