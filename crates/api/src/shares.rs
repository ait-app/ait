//! Host-owned share settings, the center's outcome, and live guest grants.
//!
//! The owner configures shares over local RPC. The daemon publishes them through
//! [`model::share::ShareBook`] and feeds the center's outcome back. Guest connections are
//! admitted only for accepted shares and end as soon as their share stops admitting them;
//! the terminals and Agent turns their guests started end with them.

mod cleanup;
pub(crate) mod guests;
pub(crate) mod rpc;
pub(crate) mod setting;

use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use model::ErrorCode;
use model::hub::{OutboxStore, StoreError};
use std::collections::BTreeSet;

use model::share::{
    Group, MAX_SHARES, MAX_VISITORS, Outcome, Publication, ResourceKind, ShareBook, Visitor,
    VisitorGrant,
};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::sync::watch;
use uuid::Uuid;

use cleanup::Owners;
use guests::{Grant, Guests, Registration, Revoked};
use setting::{SetRequest, Setting, ShareError};

/// Center code for a setting the owner revoked from another device; it becomes a local
/// tombstone that is no longer published.
const REVOKED: &str = "revoked";

/// Tombstones of revoked settings kept at most; the oldest go first.
const MAX_REVOKED: usize = MAX_SHARES;

/// Persisted share document, `<data_dir>/hub/shares.json`.
///
/// Entries are read one by one: an entry this version cannot read, such as one written
/// before shares covered groups and resource kinds, is dropped instead of failing the load.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Document {
    #[serde(default)]
    revision: u64,
    #[serde(default)]
    shares: Vec<Value>,
    #[serde(default)]
    groups: Vec<Value>,
}

/// Parse the readable entries of `values`, dropping the rest.
fn readable<T: DeserializeOwned>(values: Vec<Value>) -> Vec<T> {
    let total = values.len();
    let parsed: Vec<T> = values
        .into_iter()
        .filter_map(|value| serde_json::from_value(value).ok())
        .collect();
    if parsed.len() != total {
        tracing::warn!(
            dropped = total - parsed.len(),
            "unreadable stored share entries were dropped"
        );
    }
    parsed
}

#[derive(Debug, Default)]
struct Book {
    revision: u64,
    settings: Vec<Setting>,
    groups: Vec<Group>,
    /// Revision of the latest outcome applied; older outcomes arrive late and are ignored.
    applied: u64,
    /// Visitors the center reported as having lost access to a still accepted share.
    denied: BTreeSet<Grant>,
    /// Orders persistence of concurrent snapshots; independent of the published revision.
    version: u64,
    guests: Guests,
}

impl Book {
    fn document(&self) -> Value {
        json!({"revision": self.revision, "shares": self.settings, "groups": self.groups})
    }

    fn position(&self, kind: ResourceKind, key: &str, group_id: Uuid) -> Option<usize> {
        self.settings
            .iter()
            .position(|setting| setting.is(kind, key, group_id))
    }

    /// The setting an outcome entry is for: same resource, group and setting ID.
    fn outcome_position(
        &self,
        kind: ResourceKind,
        key: &str,
        group_id: Uuid,
        setting_id: Uuid,
    ) -> Option<usize> {
        self.position(kind, key, group_id)
            .filter(|index| self.settings[*index].id == setting_id)
    }

    /// Whether `grant` is admitted now.
    fn grants(&self, grant: Grant) -> bool {
        !self.denied.contains(&grant)
            && self
                .settings
                .iter()
                .any(|setting| setting.grants(grant.share_id, grant.user_id))
    }

    /// End grants that no accepted setting admits any more.
    fn revoke_stale(&mut self) -> Vec<Revoked> {
        let admitted: Vec<Grant> = self
            .guests
            .grants()
            .filter(|grant| self.grants(*grant))
            .collect();
        self.guests.revoke(|grant| !admitted.contains(grant))
    }
}

/// Persistence and cleanup left after one in-memory change.
#[derive(Debug, Default)]
struct Commit {
    save: Option<(u64, Value)>,
    revoked: Vec<Revoked>,
}

#[derive(Debug)]
struct Journal {
    store: Arc<dyn OutboxStore>,
    saved: Mutex<u64>,
}

impl Journal {
    /// Write `document` unless a newer snapshot was already written.
    fn write(&self, version: u64, document: &Value) -> Result<(), StoreError> {
        let mut saved = lock(&self.saved);
        if version <= *saved {
            return Ok(());
        }
        self.store.save(document)?;
        *saved = version;
        Ok(())
    }
}

/// Local share settings and live guest grants; implements [`ShareBook`].
#[derive(Debug)]
pub(crate) struct Shares {
    book: Mutex<Book>,
    changes: watch::Sender<u64>,
    /// Attached once, before serving; absent in memory-only tests.
    journal: OnceLock<Arc<Journal>>,
    owners: Owners,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shares {
    /// Create an empty, memory-only book whose revocations reach `terminal` and `provider`.
    pub(crate) fn new(
        terminal: &Arc<terminal::dispatch::State>,
        provider: &Arc<provider::dispatch::State>,
    ) -> Self {
        Self {
            book: Mutex::new(Book::default()),
            changes: watch::Sender::new(0),
            journal: OnceLock::new(),
            owners: Owners::new(terminal, provider),
        }
    }

    /// Load settings from `store` and persist every later change there. Runtime services
    /// may already hold this book, so attaching works through a shared reference.
    ///
    /// # Errors
    /// Returns the store's error for unreadable or malformed documents, or when a store
    /// is already attached.
    pub(crate) fn open(&self, store: Arc<dyn OutboxStore>) -> Result<(), StoreError> {
        let value = store.load()?;
        let document: Document = if value.is_null() {
            Document::default()
        } else {
            serde_json::from_value(value).map_err(|_| StoreError)?
        };
        self.journal
            .set(Arc::new(Journal {
                store,
                saved: Mutex::new(0),
            }))
            .map_err(|_| StoreError)?;
        let mut book = lock(&self.book);
        book.revision = document.revision;
        book.settings = readable(document.shares);
        book.groups = readable(document.groups);
        self.changes.send_replace(book.revision);
        Ok(())
    }

    /// All settings with their center status, and the owner's groups.
    pub(crate) fn list(&self) -> Value {
        let book = lock(&self.book);
        json!({"shares": book.settings, "groups": book.groups})
    }

    /// The owner's groups from the last center outcome.
    pub(crate) fn groups(&self) -> Value {
        json!({"groups": lock(&self.book).groups})
    }

    /// Admit `visitor` when an accepted share grants it, registering its connection.
    pub(crate) fn admit(self: &Arc<Self>, visitor: &Visitor) -> Option<Registration> {
        let mut book = lock(&self.book);
        let grant = Grant {
            share_id: visitor.share_id,
            user_id: visitor.user_id,
        };
        if book.denied.contains(&grant)
            || !book.settings.iter().any(|setting| setting.admits(visitor))
        {
            return None;
        }
        Some(Registration::new(self, &mut book.guests, grant))
    }

    /// After a restart, remember the grants of Agents guests created, so the center can
    /// report them as denied and their turns end. Grants no accepted setting admits any
    /// more end at once.
    pub(crate) async fn restore_guests(&self) {
        let grants = self.owners.guest_grants().await;
        if grants.is_empty() {
            return;
        }
        let ((), commit) = self.mutate(|book| {
            let restored = grants
                .into_iter()
                .take_while(|grant| book.guests.restore(*grant, MAX_VISITORS))
                .count();
            tracing::info!(restored, "guest grants restored from Agent labels");
            ((), false)
        });
        self.finish_detached(commit);
    }

    fn unregister(&self, id: u64) {
        lock(&self.book).guests.unregister(id);
    }

    fn record_terminal(&self, grant: Grant, terminal_id: String) -> bool {
        lock(&self.book).guests.record_terminal(grant, terminal_id)
    }

    fn owns_terminal(&self, grant: Grant, terminal_id: &str) -> bool {
        lock(&self.book).guests.owns_terminal(grant, terminal_id)
    }

    /// Apply `change` and revoke grants it invalidated. `change` returns whether the
    /// publishable list changed; the book is persisted either way.
    fn mutate<R>(&self, change: impl FnOnce(&mut Book) -> (R, bool)) -> (R, Commit) {
        let mut book = lock(&self.book);
        let (result, published) = change(&mut book);
        if published {
            book.revision = book.revision.saturating_add(1);
        }
        let revoked = book.revoke_stale();
        book.version = book.version.saturating_add(1);
        let save = self
            .journal
            .get()
            .is_some()
            .then(|| (book.version, book.document()));
        let revision = book.revision;
        drop(book);
        if published {
            self.changes.send_replace(revision);
        }
        (result, Commit { save, revoked })
    }

    /// Persist and clean up before returning, reporting a failed write.
    async fn finish(&self, commit: Commit) -> Result<(), ErrorCode> {
        let saved = match (commit.save, self.journal.get()) {
            (Some((version, document)), Some(journal)) => {
                let journal = journal.clone();
                tokio::task::spawn_blocking(move || journal.write(version, &document))
                    .await
                    .map_err(|_| ErrorCode::RegistryIo)?
                    .map_err(|_| ErrorCode::RegistryIo)
            }
            _ => Ok(()),
        };
        if !commit.revoked.is_empty() {
            self.owners.clone().run(commit.revoked).await;
        }
        saved
    }

    /// Persist and clean up in the background; called from synchronous ports.
    fn finish_detached(&self, commit: Commit) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            if let (Some((version, document)), Some(journal)) = (commit.save, self.journal.get())
                && journal.write(version, &document).is_err()
            {
                tracing::error!("share settings could not be saved");
            }
            return;
        };
        if let (Some((version, document)), Some(journal)) = (commit.save, self.journal.get()) {
            let journal = journal.clone();
            handle.spawn_blocking(move || {
                if journal.write(version, &document).is_err() {
                    tracing::error!("share settings could not be saved");
                }
            });
        }
        if !commit.revoked.is_empty() {
            self.owners.clone().spawn(commit.revoked, &handle);
        }
    }

    /// Create or change the share keyed by the request's resource and group.
    ///
    /// # Errors
    /// Returns invalid requests, capacity exhaustion, or a failed write.
    pub(crate) async fn set(&self, request: SetRequest) -> Result<Value, ErrorCode> {
        let (stored, commit) = self.mutate(|book| {
            let stored = upsert(book, request);
            let changed = stored.as_ref().is_ok_and(|(_, changed)| *changed);
            (stored.map(|(setting, _)| setting), changed)
        });
        let stored = stored?;
        self.finish(commit).await?;
        Ok(json!({"share": stored}))
    }

    /// Remove one share and end its guest sessions without waiting for the center.
    ///
    /// # Errors
    /// Returns a failed write after the share was removed in memory.
    pub(crate) async fn remove(
        &self,
        kind: ResourceKind,
        key: &str,
        group_id: Uuid,
    ) -> Result<Value, ErrorCode> {
        let (removed, commit) = self.mutate(|book| {
            let removed = book
                .position(kind, key, group_id)
                .map(|index| book.settings.remove(index));
            // Removing a revoked tombstone changes nothing the center sees.
            let published = removed.as_ref().is_some_and(Setting::publishable);
            (removed.is_some(), published)
        });
        self.finish(commit).await?;
        Ok(json!({
            "resourceKind": kind,
            "resourceKey": key,
            "groupId": group_id,
            "removed": removed,
        }))
    }

    /// Drop every share of retired resources of `kind` and end their guest sessions.
    ///
    /// # Arguments
    ///
    /// * `kind` - [`ResourceKind::Workspace`] for archived workspaces or
    ///   [`ResourceKind::Project`] for removed projects
    /// * `keys` - IDs of the retired resources
    pub(crate) fn forget(&self, kind: ResourceKind, keys: &[String]) {
        let ((), commit) = self.mutate(|book| {
            let before = book.settings.len();
            book.settings.retain(|setting| {
                setting.resource_kind != kind || !keys.contains(&setting.resource_key)
            });
            ((), book.settings.len() != before)
        });
        self.finish_detached(commit);
    }
}

fn upsert(book: &mut Book, request: SetRequest) -> Result<(Setting, bool), ShareError> {
    let request = request.normalize()?;
    if let Some(index) = book.position(
        request.resource_kind,
        &request.resource_key,
        request.group_id,
    ) {
        let setting = &mut book.settings[index];
        // Sharing a revoked resource again is a new setting the revocation does not cover.
        if !setting.publishable() {
            *setting = request.into_setting();
            return Ok((setting.clone(), true));
        }
        let changed = request.update(setting);
        return Ok((setting.clone(), changed));
    }
    if book.settings.iter().filter(|s| s.publishable()).count() >= MAX_SHARES {
        return Err(ShareError::Capacity);
    }
    let setting = request.into_setting();
    book.settings.push(setting.clone());
    Ok((setting, true))
}

/// Apply `outcome` unless a newer one was applied; returns whether the published list
/// changed because a setting was revoked.
/// Entries apply only to the setting they were published for, so a late outcome never
/// touches a setting the user created again.
fn apply_outcome(book: &mut Book, outcome: Outcome) -> bool {
    if outcome.revision < book.applied {
        return false;
    }
    book.applied = outcome.revision;
    for accepted in &outcome.accepted {
        if let Some(index) = book.outcome_position(
            accepted.resource_kind,
            &accepted.resource_key,
            accepted.group_id,
            accepted.setting_id,
        ) {
            book.settings[index].accept(accepted);
        }
    }
    let mut removed = false;
    for rejected in &outcome.rejected {
        let Some(index) = book.outcome_position(
            rejected.resource_kind,
            &rejected.resource_key,
            rejected.group_id,
            rejected.setting_id,
        ) else {
            continue;
        };
        if rejected.code == REVOKED {
            if book.settings[index].publishable() {
                book.settings[index].revoke();
                removed = true;
            }
        } else {
            book.settings[index].reject(&rejected.code);
        }
    }
    let revoked: Vec<usize> = (0..book.settings.len())
        .filter(|index| !book.settings[*index].publishable())
        .collect();
    for index in revoked
        .iter()
        .take(revoked.len().saturating_sub(MAX_REVOKED))
        .rev()
    {
        book.settings.remove(*index);
    }
    book.groups = outcome.groups;
    book.denied = outcome
        .denied
        .into_iter()
        .map(|denied| Grant {
            share_id: denied.share_id,
            user_id: denied.user_id,
        })
        .collect();
    removed
}

impl ShareBook for Shares {
    fn publication(&self) -> Publication {
        let book = lock(&self.book);
        Publication {
            revision: book.revision,
            shares: book
                .settings
                .iter()
                .filter(|setting| setting.publishable())
                .map(Setting::published)
                .collect(),
            visitors: book
                .guests
                .grants()
                .take(MAX_VISITORS)
                .map(|grant| VisitorGrant {
                    share_id: grant.share_id,
                    user_id: grant.user_id,
                })
                .collect(),
        }
    }

    fn changes(&self) -> watch::Receiver<u64> {
        self.changes.subscribe()
    }

    fn apply(&self, outcome: Outcome) {
        let ((), commit) = self.mutate(|book| ((), apply_outcome(book, outcome)));
        self.finish_detached(commit);
    }
}

#[cfg(test)]
mod tests;
