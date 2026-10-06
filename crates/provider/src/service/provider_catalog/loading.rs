use super::{AgentClient, SessionEvents, Value, json, watch};
use super::{Arc, BTreeMap, CancellationToken, Duration, ErrorCode, Instant, Semaphore};
use super::{Cache, Catalog, Entry, Snapshot, discover, response};

#[derive(Clone, Copy)]
pub(super) struct Scope<'a> {
    pub(super) key: &'a Option<String>,
    pub(super) cwd: &'a str,
    pub(super) providers: &'a [String],
    pub(super) refresh: bool,
}

struct Load {
    key: Option<String>,
    provider: String,
    cwd: String,
    client: Arc<dyn AgentClient>,
    limit: Arc<Semaphore>,
    cancellation: CancellationToken,
    generation: u64,
    completion: watch::Sender<bool>,
    _pending: tokio::sync::OwnedSemaphorePermit,
    queued: Instant,
}

impl Catalog {
    pub(super) fn start(
        &self,
        clients: &BTreeMap<String, Arc<dyn AgentClient>>,
        events: &SessionEvents,
        scope: Scope<'_>,
    ) -> Result<Vec<watch::Receiver<bool>>, ErrorCode> {
        let mut cache = self.cache.lock().map_err(|_| ErrorCode::AgentIo)?;
        prepare(&mut cache, scope.key.as_ref(), clients, &self.cancellation);
        let mut waits = Vec::with_capacity(scope.providers.len());
        for provider in scope.providers {
            let limit = cache
                .limits
                .entry(provider.clone())
                .or_insert_with(|| Arc::new(Semaphore::new(4)))
                .clone();
            cache.generation += 1;
            let generation = cache.generation;
            let snapshot = cache
                .snapshots
                .get_mut(scope.key)
                .ok_or(ErrorCode::AgentIo)?;
            let entry = snapshot
                .entries
                .get_mut(provider)
                .ok_or(ErrorCode::UnsupportedCapability)?;
            if let Some(wait) = &entry.loading {
                entry.refresh_again |= scope.refresh;
                waits.push(wait.clone());
                continue;
            }
            if !scope.refresh
                && entry
                    .fetched
                    .is_some_and(|at| at.elapsed() < Duration::from_secs(60))
            {
                continue;
            }
            let pending = self
                .pending
                .clone()
                .try_acquire_owned()
                .map_err(|_| ErrorCode::CatalogBusy)?;
            let (completion, wait) = watch::channel(false);
            entry.loading = Some(wait.clone());
            entry.generation = generation;
            snapshot.revision = generation;
            waits.push(wait);
            let load = Load {
                key: scope.key.clone(),
                provider: provider.clone(),
                cwd: scope.cwd.to_owned(),
                client: clients
                    .get(provider)
                    .ok_or(ErrorCode::UnsupportedCapability)?
                    .clone(),
                limit,
                cancellation: snapshot.cancellation.child_token(),
                generation,
                completion,
                _pending: pending,
                queued: Instant::now(),
            };
            let catalog = self.clone();
            let events = events.clone();
            self.tasks.spawn(async move {
                run(catalog, load, events).await;
            });
        }
        Ok(waits)
    }
}

fn prepare(
    cache: &mut Cache,
    key: Option<&String>,
    clients: &BTreeMap<String, Arc<dyn AgentClient>>,
    cancellation: &CancellationToken,
) {
    let key = key.cloned();
    if cache.snapshots.len() >= 16 && !cache.snapshots.contains_key(&key) {
        let oldest = cache
            .snapshots
            .iter()
            .min_by_key(|(_, snapshot)| snapshot.fetched)
            .map(|(key, _)| key.clone());
        if let Some(oldest) = oldest.and_then(|key| cache.snapshots.remove(&key)) {
            oldest.cancellation.cancel();
        }
    }
    let epoch = cache.epoch.clone();
    let revision = cache.generation;
    cache.snapshots.entry(key).or_insert_with(|| Snapshot {
        entries: clients
            .keys()
            .map(|provider| (provider.clone(), initial(provider)))
            .collect(),
        fetched: Instant::now(),
        cancellation: cancellation.child_token(),
        epoch,
        revision,
    });
}

fn initial(provider: &str) -> Entry {
    Entry {
        value: json!({"provider":provider,"status":"loading","enabled":true,
            "source":"builtin","models":[],"modes":[],"fetchedAt":chrono::Utc::now().to_rfc3339()}),
        features: Vec::new(),
        fetched: None,
        generation: 0,
        loading: None,
        refresh_again: false,
    }
}

async fn run(catalog: Catalog, load: Load, events: SessionEvents) {
    let mut queued = load.queued;
    loop {
        let probe = async {
            let _provider = load.limit.acquire().await;
            let _global = catalog.budget.acquire().await;
            let queue_ms = queued.elapsed().as_millis();
            let started = Instant::now();
            let result = discover(load.client.as_ref(), &load.cwd).await;
            tracing::debug!(
                provider = load.provider,
                class = "catalog",
                queue_ms,
                native_ms = started.elapsed().as_millis(),
                active = 16 - catalog.budget.available_permits(),
                "provider.discovery.completed"
            );
            result
        };
        let result = tokio::select! {
            () = load.cancellation.cancelled() => break,
            result = tokio::time::timeout(Duration::from_secs(30), probe) => result,
        };
        let result = result.unwrap_or_else(|_| failure(&load.provider));
        let Some((snapshot, again)) = commit(&catalog, &load, result) else {
            break;
        };
        let _ = response(
            &snapshot,
            &events,
            "provider.snapshot.refresh.request",
            super::ReplyScope {
                key: load.key.clone(),
                selected: None,
                if_none_match: None,
                refresh: true,
            },
        );
        if !again {
            break;
        }
        queued = Instant::now();
    }
    load.completion.send_replace(true);
}

fn commit(catalog: &Catalog, load: &Load, mut result: Entry) -> Option<(Snapshot, bool)> {
    let mut cache = catalog.cache.lock().ok()?;
    cache.generation += 1;
    let revision = cache.generation;
    let snapshot = cache.snapshots.get_mut(&load.key)?;
    let current = snapshot.entries.get(&load.provider)?;
    if current.generation != load.generation || load.cancellation.is_cancelled() {
        return None;
    }
    let again = current.refresh_again;
    result.generation = load.generation;
    if again {
        result.loading.clone_from(&current.loading);
    }
    snapshot.entries.insert(load.provider.clone(), result);
    snapshot.fetched = Instant::now();
    snapshot.revision = revision;
    Some((snapshot.clone(), again))
}

fn failure(provider: &str) -> Entry {
    let mut entry = initial(provider);
    entry.value["status"] = Value::String("error".to_owned());
    entry.value["error"] = Value::String("Provider discovery timed out".to_owned());
    entry.fetched = Some(Instant::now());
    entry
}
