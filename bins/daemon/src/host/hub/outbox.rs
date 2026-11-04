//! Daemon publication journal. Mutations become visible only after an atomic store commit.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use anyhow::Context;
use model::hub::{OutboxStore, Scope};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

pub(super) const EVENTS: &str = "hub.events.publish";
pub(super) const DIRECTORY: &str = "hub.directory.publish";
const MAX_EVENTS: usize = 1024;
const MAX_SEEN: usize = 4096;
const MAX_BYTES: usize = 24 * 1024 * 1024;
const RETENTION_MS: i64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Entry {
    pub(super) id: Uuid,
    pub(super) params: Value,
    created_at: i64,
    blocked: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Snapshot {
    revision: u64,
    entry: Entry,
    pending: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Document {
    version: u8,
    scope: Option<Scope>,
    events: VecDeque<Entry>,
    directories: BTreeMap<String, Snapshot>,
    seen: BTreeMap<String, i64>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            version: 1,
            scope: None,
            events: VecDeque::new(),
            directories: BTreeMap::new(),
            seen: BTreeMap::new(),
        }
    }
}

/// The event journal reached its entry limit; the event is dropped rather than retried.
#[derive(Debug, thiserror::Error)]
#[error("Host event outbox full")]
pub(super) struct OutboxFull;

/// Immutable delivery attempt; a concurrent enqueue cannot change its event ID or content.
#[derive(Debug, Clone)]
pub(super) struct Job {
    pub(super) scope: Scope,
    pub(super) method: &'static str,
    pub(super) entry: Entry,
}

pub(super) struct Outbox {
    store: Arc<dyn OutboxStore>,
    document: Document,
}

impl Outbox {
    pub(super) fn open(store: Arc<dyn OutboxStore>) -> anyhow::Result<Self> {
        let value = store.load().context("read Host publication journal")?;
        let document: Document = if value.is_null() {
            Document::default()
        } else {
            serde_json::from_value(value).context("decode Host publication journal")?
        };
        anyhow::ensure!(
            document.version == 1,
            "unsupported Host publication journal version"
        );
        Ok(Self { store, document })
    }

    fn commit(&mut self, document: Document) -> anyhow::Result<()> {
        let bytes = serde_json::to_vec(&document)?;
        anyhow::ensure!(bytes.len() <= MAX_BYTES, "Host publication journal full");
        self.store
            .save(&bytes)
            .context("commit Host publication journal")?;
        self.document = document;
        Ok(())
    }

    pub(super) fn activate(&mut self, scope: &Scope, now: i64) -> anyhow::Result<()> {
        if self.document.scope.as_ref() != Some(scope) {
            return self.commit(Document {
                scope: Some(scope.clone()),
                ..Document::default()
            });
        }
        let retained = |created: i64| now.saturating_sub(created) < RETENTION_MS;
        if self
            .document
            .events
            .iter()
            .all(|entry| retained(entry.created_at))
            && self
                .document
                .seen
                .values()
                .all(|created| retained(*created))
        {
            return Ok(());
        }
        let mut next = self.document.clone();
        next.events.retain(|entry| retained(entry.created_at));
        next.seen.retain(|_, created| retained(*created));
        self.commit(next)
    }

    pub(super) fn enqueue(&mut self, scope: &Scope, params: Value, now: i64) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.document.scope.as_ref() == Some(scope),
            "publication authority changed"
        );
        let key = attention_key(&params);
        if key
            .as_ref()
            .is_some_and(|key| self.document.seen.contains_key(key))
        {
            return Ok(());
        }
        if self.document.events.len() >= MAX_EVENTS {
            return Err(OutboxFull.into());
        }
        let mut next = self.document.clone();
        if let Some(key) = key {
            while next.seen.len() >= MAX_SEEN {
                let oldest = next
                    .seen
                    .iter()
                    .min_by_key(|(_, at)| **at)
                    .map(|(key, _)| key.clone());
                if let Some(oldest) = oldest {
                    next.seen.remove(&oldest);
                }
            }
            next.seen.insert(key, now);
        }
        let id = serde_json::from_value(params["eventId"].clone())?;
        next.events.push_back(Entry {
            id,
            params,
            created_at: now,
            blocked: None,
        });
        self.commit(next)
    }

    pub(super) fn snapshot(
        &mut self,
        kind: &str,
        items: Value,
        force: bool,
        now: i64,
    ) -> anyhow::Result<()> {
        if let Some(previous) = self.document.directories.get(kind)
            && previous.entry.params["items"] == items
        {
            if force && !previous.pending {
                let mut next = self.document.clone();
                if let Some(snapshot) = next.directories.get_mut(kind) {
                    snapshot.pending = true;
                }
                self.commit(next)?;
            }
            return Ok(());
        }
        let revision = self
            .document
            .directories
            .get(kind)
            .map_or(Ok(1), |previous| {
                previous
                    .revision
                    .checked_add(1)
                    .context("directory revision exhausted")
            })?;
        let id = Uuid::new_v4();
        let mut params = json!({"kind":kind,"revision":revision,"eventId":id});
        params["items"] = items;
        anyhow::ensure!(
            serde_json::to_vec(&params)?.len() <= 1024 * 1024,
            "Host directory exceeds center budget"
        );
        let mut next = self.document.clone();
        next.directories.insert(
            kind.to_owned(),
            Snapshot {
                revision,
                entry: Entry {
                    id,
                    params,
                    created_at: now,
                    blocked: None,
                },
                pending: true,
            },
        );
        self.commit(next)
    }

    /// Clear quarantine so a fresh logical link retries every retained entry once.
    /// Rejections such as `unauthorized` can be specific to the link that returned them.
    pub(super) fn unblock(&mut self) -> anyhow::Result<()> {
        let blocked = self
            .document
            .events
            .iter()
            .any(|entry| entry.blocked.is_some())
            || self
                .document
                .directories
                .values()
                .any(|snapshot| snapshot.entry.blocked.is_some());
        if !blocked {
            return Ok(());
        }
        let mut next = self.document.clone();
        for entry in &mut next.events {
            entry.blocked = None;
        }
        for snapshot in next.directories.values_mut() {
            if snapshot.entry.blocked.take().is_some() {
                snapshot.pending = true;
            }
        }
        self.commit(next)
    }

    /// The next deliverable entry using only `methods` the current link advertises, so an
    /// unsupported event cannot hold back directory snapshots behind it.
    pub(super) fn next(&self, scope: &Scope, methods: &[String], now: i64) -> Option<Job> {
        if self.document.scope.as_ref() != Some(scope) {
            return None;
        }
        let supports = |method: &str| methods.iter().any(|available| available == method);
        let event = supports(EVENTS)
            .then(|| {
                self.document.events.iter().find(|entry| {
                    entry.blocked.is_none() && now.saturating_sub(entry.created_at) < RETENTION_MS
                })
            })
            .flatten()
            .map(|entry| (EVENTS, entry));
        let (method, entry) = event.or_else(|| {
            supports(DIRECTORY)
                .then(|| {
                    self.document
                        .directories
                        .values()
                        .find(|snapshot| snapshot.pending && snapshot.entry.blocked.is_none())
                })
                .flatten()
                .map(|snapshot| (DIRECTORY, &snapshot.entry))
        })?;
        Some(Job {
            scope: scope.clone(),
            method,
            entry: entry.clone(),
        })
    }

    pub(super) fn complete(&mut self, job: &Job, blocked: Option<String>) -> anyhow::Result<()> {
        if self.document.scope.as_ref() != Some(&job.scope) {
            return Ok(());
        }
        let mut next = self.document.clone();
        if job.method == EVENTS {
            if let Some(code) = blocked {
                if let Some(entry) = next
                    .events
                    .iter_mut()
                    .find(|entry| entry.id == job.entry.id)
                {
                    entry.blocked = Some(code);
                }
            } else {
                next.events.retain(|entry| entry.id != job.entry.id);
            }
        } else {
            for snapshot in next
                .directories
                .values_mut()
                .filter(|snapshot| snapshot.entry.id == job.entry.id)
            {
                snapshot.pending = blocked.is_some();
                snapshot.entry.blocked.clone_from(&blocked);
            }
        }
        self.commit(next)
    }
}

fn attention_key(params: &Value) -> Option<String> {
    if params["method"] != "agent_attention_required" {
        return None;
    }
    let body = &params["params"];
    Some(
        json!([
            body["agentId"].as_str()?,
            body["timestamp"].as_str()?,
            body["reason"].as_str()?
        ])
        .to_string(),
    )
}

#[cfg(test)]
mod tests;
