//! Independent, bounded Provider discovery and scope-aware snapshots.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::Utc;
use metadata::protocol::session::SessionEventKind;
use metadata::service::session::SessionEvents;
use model::ErrorCode;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::{Semaphore, watch};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::ports::agent_session::AgentClient;
use crate::protocol::provider::{Details, ListRequest, RefreshRequest, SnapshotRequest};

mod draft;
mod loading;
mod scope;

#[derive(Debug, Clone)]
struct Entry {
    value: Value,
    features: Vec<Value>,
    fetched: Option<Instant>,
    generation: u64,
    loading: Option<watch::Receiver<bool>>,
    refresh_again: bool,
}

#[derive(Debug, Clone)]
struct Snapshot {
    entries: BTreeMap<String, Entry>,
    fetched: Instant,
    cancellation: CancellationToken,
    epoch: String,
    revision: u64,
}

#[derive(Debug)]
struct Cache {
    snapshots: BTreeMap<Option<String>, Snapshot>,
    limits: BTreeMap<String, Arc<Semaphore>>,
    generation: u64,
    epoch: String,
}

impl Default for Cache {
    fn default() -> Self {
        Self {
            snapshots: BTreeMap::new(),
            limits: BTreeMap::new(),
            generation: 0,
            epoch: uuid::Uuid::new_v4().to_string(),
        }
    }
}

/// Shared discovery owner. Native probes never occupy an Agent command lane.
#[derive(Debug, Clone)]
pub(crate) struct Catalog {
    cache: Arc<Mutex<Cache>>,
    budget: Arc<Semaphore>,
    pending: Arc<Semaphore>,
    cancellation: CancellationToken,
    tasks: TaskTracker,
}

impl Default for Catalog {
    fn default() -> Self {
        Self {
            cache: Arc::default(),
            budget: Arc::new(Semaphore::new(16)),
            pending: Arc::new(Semaphore::new(64)),
            cancellation: CancellationToken::new(),
            tasks: TaskTracker::new(),
        }
    }
}

impl Catalog {
    pub(crate) async fn execute(
        &self,
        clients: &BTreeMap<String, Arc<dyn AgentClient>>,
        events: &SessionEvents,
        method: &str,
        params: Value,
    ) -> Result<Value, ErrorCode> {
        self.request(clients, events, method, params, true).await
    }

    /// Read current snapshots immediately; legacy list methods wait only for their discovery.
    /// # Errors
    /// Rejects invalid scopes, unsupported providers, shutdown and unavailable storage.
    pub(crate) async fn read(
        &self,
        clients: &BTreeMap<String, Arc<dyn AgentClient>>,
        events: &SessionEvents,
        method: &str,
        params: Value,
    ) -> Result<Value, ErrorCode> {
        self.request(clients, events, method, params, false).await
    }

    async fn request(
        &self,
        clients: &BTreeMap<String, Arc<dyn AgentClient>>,
        events: &SessionEvents,
        method: &str,
        params: Value,
        wait_snapshot: bool,
    ) -> Result<Value, ErrorCode> {
        if self.cancellation.is_cancelled() {
            return Err(ErrorCode::ServerDraining);
        }
        if method == "provider.features.list.request" && params.get("draftConfig").is_some() {
            return draft::features(clients, decode(params)?).await;
        }
        let scope = parse(clients, method, params)?;
        let cwd = scope::discovery_cwd(scope.key.as_deref())?;
        let selected: Vec<_> = scope
            .selected
            .clone()
            .unwrap_or_else(|| clients.keys().cloned().collect());
        let waits = self.start(
            clients,
            events,
            loading::Scope {
                key: &scope.key,
                cwd: &cwd,
                providers: &selected,
                refresh: scope.refresh,
            },
        )?;
        if wait_snapshot || !method.starts_with("provider.snapshot.") {
            for mut wait in waits {
                tokio::select! {
                    () = self.cancellation.cancelled() => return Err(ErrorCode::ServerDraining),
                    _ = wait.wait_for(|done| *done) => {},
                }
            }
        }
        let snapshot = self
            .cache
            .lock()
            .map_err(|_| ErrorCode::AgentIo)?
            .snapshots
            .get(&scope.key)
            .cloned()
            .ok_or(ErrorCode::CatalogBusy)?;
        response(&snapshot, events, method, scope)
    }

    /// Cancel discovery and retain ownership until every accepted task has stopped.
    pub(crate) async fn close(&self) {
        self.cancellation.cancel();
        self.tasks.close();
        self.tasks.wait().await;
    }
}

fn parse(
    clients: &BTreeMap<String, Arc<dyn AgentClient>>,
    method: &str,
    params: Value,
) -> Result<ReplyScope, ErrorCode> {
    let (cwd, selected, if_none_match, refresh) = match method {
        "provider.available.list.request" => {
            crate::rpc::agent_execution::only(&params, &[])?;
            (None, None, None, false)
        }
        "provider.snapshot.get.request" => {
            let request: SnapshotRequest = decode(params)?;
            (request.cwd, None, request.if_none_match, false)
        }
        "provider.snapshot.refresh.request" => {
            let request: RefreshRequest = decode(params)?;
            (request.cwd, request.providers, None, true)
        }
        _ => {
            let request: ListRequest = decode(params)?;
            (request.cwd, Some(vec![request.provider]), None, false)
        }
    };
    if selected.as_ref().is_some_and(|providers| {
        providers.len() > 32
            || providers
                .iter()
                .any(|provider| !clients.contains_key(provider))
    }) {
        return Err(ErrorCode::UnsupportedCapability);
    }
    Ok(ReplyScope {
        key: scope::key(cwd.as_deref())?,
        selected,
        if_none_match,
        refresh,
    })
}

async fn discover(client: &dyn AgentClient, cwd: &str) -> Entry {
    let available = client.is_available().await;
    let (status, error, details) = if matches!(available, Ok(true)) {
        match client.discover(cwd).await {
            Ok(details) => ("ready", None, details),
            Err(_) => (
                "error",
                Some("Provider discovery failed"),
                Details::default(),
            ),
        }
    } else {
        (
            "unavailable",
            Some("Provider executable is unavailable"),
            Details::default(),
        )
    };
    let mut value = json!({"provider":client.provider(),"status":status,"enabled":true,"source":"builtin",
        "models":details.models,"modes":details.modes,"fetchedAt":Utc::now().to_rfc3339()});
    if client.provider() == "deepseek-harness" {
        value["label"] = json!("DeepSeek Harness");
        value["description"] = json!(if client
            .settings(&domain::agent_runtime::StoredAgentConfig::default())["capabilities"]["supportsDynamicModes"]
            == true
        {
            "DeepSeek Harness native interactive Host"
        } else {
            "DeepSeek Harness via Agent Client Protocol"
        });
        value["defaultModeId"] = Value::Null;
    }
    if client.provider() == "antigravity" {
        value["label"] = json!("Antigravity");
        value["description"] = json!("Google Antigravity via the official AGY CLI");
        value["defaultModeId"] = json!("default");
    }
    if let Some(error) = error {
        value["error"] = json!(error);
    }
    Entry {
        value,
        features: details.features,
        fetched: Some(Instant::now()),
        generation: 0,
        loading: None,
        refresh_again: false,
    }
}

fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, ErrorCode> {
    serde_json::from_value(value).map_err(|_| ErrorCode::InvalidMessage)
}

#[cfg(test)]
mod tests;

struct ReplyScope {
    key: Option<String>,
    selected: Option<Vec<String>>,
    if_none_match: Option<String>,
    refresh: bool,
}

fn response(
    snapshot: &Snapshot,
    events: &SessionEvents,
    method: &str,
    scope: ReplyScope,
) -> Result<Value, ErrorCode> {
    let ReplyScope {
        key,
        selected,
        if_none_match,
        refresh,
    } = scope;

    let entries: Vec<_> = snapshot
        .entries
        .values()
        .map(|entry| entry.value.clone())
        .collect();
    let fetched_at = Utc::now().to_rfc3339();
    if method == "provider.available.list.request" {
        let providers: Vec<_> = entries.iter().map(|entry|json!({"provider":entry["provider"],"available":entry["status"]=="ready","error":entry.get("error")})).collect();
        return Ok(json!({"providers":providers,"error":null,"fetchedAt":fetched_at}));
    }
    if matches!(
        method,
        "provider.snapshot.get.request" | "provider.snapshot.refresh.request"
    ) {
        let hashable: Vec<_> = entries
            .iter()
            .map(|entry| {
                let mut entry = entry.clone();
                if let Some(object) = entry.as_object_mut() {
                    object.remove("fetchedAt");
                }
                entry
            })
            .collect();
        let bytes = serde_json::to_vec(&hashable).map_err(|_| ErrorCode::AgentIo)?;
        let hash = format!("{:x}", Sha256::digest(bytes));
        let refreshing: Vec<_> = snapshot
            .entries
            .iter()
            .filter(|(_, entry)| entry.loading.is_some())
            .map(|(provider, _)| provider.as_str())
            .collect();
        let mut payload = json!({"entries":entries,"snapshotHash":hash,"generatedAt":fetched_at,
            "generation":snapshot.epoch,"revision":snapshot.revision,"refreshing":refreshing});
        if let Some(key) = key {
            payload["cwd"] = json!(key);
        }
        if refresh {
            events.publish(SessionEventKind::ProvidersSnapshot, &payload);
            return Ok(
                json!({"acknowledged":true,"generation":snapshot.epoch,"revision":snapshot.revision}),
            );
        }
        let unchanged = if_none_match.as_deref() == Some(&hash);
        payload["notModified"] = json!(unchanged);
        if unchanged {
            payload["entries"] = json!([]);
        }
        return crate::rpc::timeline::bounded(payload);
    }
    let provider = selected
        .as_ref()
        .and_then(|values| values.first())
        .ok_or(ErrorCode::InvalidMessage)?;
    let entry = snapshot
        .entries
        .get(provider)
        .ok_or(ErrorCode::UnsupportedCapability)?;
    let field = match method {
        "provider.models.list.request" => "models",
        "provider.modes.list.request" => "modes",
        "provider.features.list.request" => "features",
        _ => return Err(ErrorCode::MethodNotFound),
    };
    let values = if field == "features" {
        json!(entry.features)
    } else if field == "models" {
        json!(
            entry.value[field]
                .as_array()
                .ok_or(ErrorCode::AgentIo)?
                .iter()
                .filter(|model| model.get("isSelectable") != Some(&Value::Bool(false)))
                .collect::<Vec<_>>()
        )
    } else {
        entry.value[field].clone()
    };
    crate::rpc::timeline::bounded(
        json!({"provider":provider,(field):values,"error":entry.value.get("error"),"fetchedAt":entry.value["fetchedAt"]}),
    )
}
