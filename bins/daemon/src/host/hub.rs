//! Host publication application: bounded ingress, durable retry and directory projection.

mod events;
mod outbox;
pub(super) mod sources;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use chrono::Utc;
use model::hub::{CallError, Center, EventPublisher, LinkState, Scope};
use serde_json::Value;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, watch};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use events::Event;
use outbox::{DIRECTORY, EVENTS, Job, Outbox};
use sources::Sources;

const INGRESS_EVENTS: usize = 256;
const INGRESS_BYTES: usize = 4 * 1024 * 1024;
const RETRY: Duration = Duration::from_secs(2);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
const COALESCE: Duration = Duration::from_millis(250);

struct Incoming {
    scope: Scope,
    event: Event,
    _bytes: OwnedSemaphorePermit,
}

#[derive(Debug)]
struct Ingress {
    state: watch::Receiver<LinkState>,
    queue: mpsc::Sender<Incoming>,
    bytes: Arc<Semaphore>,
    changes: model::changes::Changes,
    native: bool,
}

impl EventPublisher for Ingress {
    fn publish(&self, method: &str, params: &Value) -> bool {
        let state = self.state.borrow();
        if !state.supports(EVENTS) {
            return false;
        }
        let Some(scope) = state.scope.clone() else {
            return false;
        };
        let Some(event) = Event::project(method, params) else {
            return false;
        };
        let attention = event.attention();
        let size = serde_json::to_vec(&event.params).map_or(usize::MAX, |bytes| bytes.len());
        let permit = u32::try_from(size)
            .ok()
            .and_then(|size| self.bytes.clone().try_acquire_many_owned(size).ok());
        let accepted = permit.is_some_and(|bytes| {
            self.queue
                .try_send(Incoming {
                    scope,
                    event,
                    _bytes: bytes,
                })
                .is_ok()
        });
        if !accepted {
            self.changes.notify();
            tracing::error!(
                "Host publication ingress full or closed; resynchronizing committed state"
            );
        }
        // Selecting the center route must never also invoke legacy APNs/FCM.
        attention && self.native
    }
}

struct Application {
    center: Arc<dyn Center>,
    sources: Sources,
    server: String,
    outbox: Arc<Mutex<Outbox>>,
    changed: tokio::sync::Notify,
    cancel: CancellationToken,
    native: bool,
}

/// Owns publication workers through shutdown, including the final ingress flush.
pub(super) struct Worker {
    cancel: CancellationToken,
    tasks: TaskTracker,
}

impl Worker {
    pub(super) async fn shutdown(&self) {
        self.cancel.cancel();
        self.tasks.wait().await;
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

pub(super) async fn start(
    api: &api::Api,
    sources: Sources,
    data: &std::path::Path,
    lifetime: Arc<crate::instance::InstanceLease>,
    native: bool,
) -> anyhow::Result<Worker> {
    let store = Arc::new(persistence::storage::hub::FileOutbox::new(
        data.join("hub/publications.json"),
    ));
    let outbox = blocking(move || Outbox::open(store)).await?;
    let center = api.hub_center();
    let (sender, receiver) = mpsc::channel(INGRESS_EVENTS);
    let ingress = Ingress {
        state: center.state(),
        queue: sender,
        bytes: Arc::new(Semaphore::new(INGRESS_BYTES)),
        changes: sources.changes.clone(),
        native,
    };
    let cancel = CancellationToken::new();
    let tasks = TaskTracker::new();
    let application = Arc::new(Application {
        center,
        sources,
        server: lifetime.server_id.to_string(),
        outbox: Arc::new(Mutex::new(outbox)),
        changed: tokio::sync::Notify::new(),
        cancel: cancel.clone(),
        native,
    });
    let spool = application.clone();
    let spool_lifetime = lifetime.clone();
    tasks.spawn(async move {
        spool.spool(receiver).await;
        drop(spool_lifetime);
    });
    tasks.spawn(async move {
        application.deliver().await;
        drop(lifetime);
    });
    tasks.close();
    api.set_event_publisher(Arc::new(ingress));
    Ok(Worker { cancel, tasks })
}

impl Application {
    async fn spool(self: Arc<Self>, mut incoming: mpsc::Receiver<Incoming>) {
        let mut state = self.center.state();
        let mut changes = self.sources.changes.subscribe();
        let mut dirty = true;
        let mut last_link = None;
        let mut reconcile_at = tokio::time::Instant::now();
        loop {
            tokio::select! {
                biased;
                () = self.cancel.cancelled() => {
                    incoming.close();
                    while let Ok(event) = incoming.try_recv() {
                        if let Err(error) = self.store_event(&event).await {
                            tracing::error!(%error, "Host ingress flush failed at shutdown");
                        }
                    }
                    return;
                }
                result = state.changed() => {
                    if result.is_err() { return; }
                    dirty = true;
                    reconcile_at = tokio::time::Instant::now();
                }
                () = tokio::time::sleep_until(reconcile_at), if dirty => {
                    let current = state.borrow().clone();
                    let force = current.online && current.link != last_link;
                    match self.reconcile(current.clone(), force).await {
                        Ok(()) => { dirty = false; if current.online { last_link = current.link; } }
                        Err(error) => {
                            tracing::warn!(%error, "Host publication reconciliation will retry");
                            reconcile_at = tokio::time::Instant::now() + RETRY;
                        }
                    }
                }
                Some(event) = incoming.recv() => {
                    while let Err(error) = self.store_event(&event).await {
                        tracing::warn!(%error, "Host publication journal enqueue will retry");
                        tokio::select! {
                            () = self.cancel.cancelled() => {
                                if let Err(error) = self.store_event(&event).await {
                                    tracing::error!(%error, "Host event flush failed at shutdown");
                                }
                                break;
                            }
                            () = tokio::time::sleep(RETRY) => {}
                        }
                    }
                }
                result = changes.changed() => {
                    if result.is_err() { return; }
                    if !dirty { reconcile_at = tokio::time::Instant::now() + COALESCE; }
                    dirty = true;
                }
            }
        }
    }

    async fn store_event(&self, incoming: &Incoming) -> anyhow::Result<()> {
        let current = self.center.state().borrow().clone();
        if current.scope.as_ref() != Some(&incoming.scope) {
            return Ok(());
        }
        let sources = self.sources.clone();
        let server = self.server.clone();
        let event = Event {
            method: incoming.event.method.clone(),
            params: incoming.event.params.clone(),
        };
        let outbox = self.outbox.clone();
        let scope = incoming.scope.clone();
        let native = self.native;
        blocking(move || {
            let publication = event.publication(&sources, &server, native)?;
            let mut outbox = outbox
                .lock()
                .map_err(|_| anyhow::anyhow!("Host publication journal poisoned"))?;
            let now = Utc::now().timestamp_millis();
            outbox.activate(&scope, now)?;
            outbox.enqueue(&scope, publication, now)
        })
        .await?;
        self.changed.notify_one();
        Ok(())
    }

    async fn reconcile(&self, state: LinkState, force: bool) -> anyhow::Result<()> {
        let Some(scope) = state.scope.clone() else {
            return Ok(());
        };
        let sources = self.sources.clone();
        let server = self.server.clone();
        let outbox = self.outbox.clone();
        let native = self.native;
        blocking(move || {
            let now = Utc::now().timestamp_millis();
            let mut outbox = outbox
                .lock()
                .map_err(|_| anyhow::anyhow!("Host publication journal poisoned"))?;
            outbox.activate(&scope, now)?;
            if state.supports(DIRECTORY) {
                for (kind, items) in sources.snapshots()? {
                    outbox.snapshot(kind, items, force, now)?;
                }
            }
            if state.supports(EVENTS) {
                for params in sources.attention(now)? {
                    let event = Event {
                        method: "agent_attention_required".to_owned(),
                        params,
                    };
                    outbox.enqueue(&scope, event.publication(&sources, &server, native)?, now)?;
                }
            }
            Ok(())
        })
        .await?;
        self.changed.notify_one();
        Ok(())
    }

    async fn deliver(&self) {
        let mut state = self.center.state();
        let mut delay = Duration::ZERO;
        let mut backoff = RETRY;
        loop {
            let current = state.borrow().clone();
            if current.online && delay.is_zero() {
                match self.attempt(current).await {
                    Ok(true) => {
                        backoff = RETRY;
                        continue;
                    }
                    Ok(false) => {}
                    Err(error) => {
                        tracing::warn!(%error, "Host publication delivery will retry");
                        delay = backoff;
                        backoff = backoff.saturating_mul(2).min(MAX_BACKOFF);
                    }
                }
            }
            tokio::select! {
                biased;
                () = self.cancel.cancelled() => return,
                result = state.changed() => {
                    if result.is_err() { return; }
                    delay = Duration::ZERO;
                }
                () = tokio::time::sleep(delay), if !delay.is_zero() => delay = Duration::ZERO,
                () = self.changed.notified(), if delay.is_zero() => {}
            }
        }
    }

    async fn attempt(&self, state: LinkState) -> anyhow::Result<bool> {
        let Some(scope) = state.scope else {
            return Ok(false);
        };
        let outbox = self.outbox.clone();
        let job = blocking(move || {
            let outbox = outbox
                .lock()
                .map_err(|_| anyhow::anyhow!("Host publication journal poisoned"))?;
            Ok(outbox.next(&scope, Utc::now().timestamp_millis()))
        })
        .await?;
        let Some(job) = job else {
            return Ok(false);
        };
        if !state.methods.iter().any(|method| method == job.method) {
            return Ok(false);
        }
        let sources = self.sources.clone();
        let server = self.server.clone();
        let params = job.entry.params.clone();
        if job.method == EVENTS
            && !blocking(move || Event::current_notification(&params, &sources, &server)).await?
        {
            let outbox = self.outbox.clone();
            blocking(move || {
                outbox
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Host publication journal poisoned"))?
                    .complete(&job, None)
            })
            .await?;
            return Ok(true);
        }
        let result = tokio::select! {
            () = self.cancel.cancelled() => return Ok(false),
            result = self.center.call(
                job.scope.clone(), job.method, job.entry.params.clone()
            ) => result,
        };
        let blocked = match result {
            Ok(value) => {
                validate_result(&job, &value)?;
                None
            }
            Err(CallError::Rejected(code)) if permanent(&code) => {
                tracing::error!(
                    code,
                    method = job.method,
                    "Host publication quarantined; inspect center compatibility"
                );
                Some(code)
            }
            Err(error) => return Err(error.into()),
        };
        let outbox = self.outbox.clone();
        blocking(move || {
            outbox
                .lock()
                .map_err(|_| anyhow::anyhow!("Host publication journal poisoned"))?
                .complete(&job, blocked)
        })
        .await?;
        Ok(true)
    }
}

fn permanent(code: &str) -> bool {
    matches!(
        code,
        "invalid_params"
            | "invalid_request"
            | "bad_request"
            | "conflict"
            | "not_found"
            | "forbidden"
            | "unauthorized"
            | "method_not_found"
    )
}

fn validate_result(job: &Job, value: &Value) -> anyhow::Result<()> {
    let valid = if job.method == EVENTS {
        value["cursor"].is_u64() && value["inserted"].is_boolean()
    } else {
        value["revision"] == job.entry.params["revision"]
    };
    anyhow::ensure!(valid, "invalid center publication result");
    Ok(())
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> anyhow::Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .context("join Host publication storage task")?
}

#[cfg(test)]
mod tests;
