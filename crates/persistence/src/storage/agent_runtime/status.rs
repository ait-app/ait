//! In-memory `agents.json` document with deferred status writes (ADR-288).
//!
//! Reads are served from memory. Structural mutations replace the file before they return;
//! status mutations only mark the document dirty. A background flusher writes dirty content
//! every [`FLUSH_INTERVAL`]; [`AgentsStatus::flush`] writes it on demand and dropping the last
//! owner writes it once more. [`AgentsStatus::close`] writes a final document marked `closed`
//! and rejects later mutations. A document that was not closed records when its content last
//! changed, so startup recovery knows which timeline events it may lack.

use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};
use std::time::Duration;

use chrono::{SecondsFormat, Utc};
use domain::agent_runtime::PersistedAgentRuntimeRecord;
use domain::agent_runtime::registry::AgentRuntimeRegistryError as Error;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize, Serializer};

use crate::registry::MAX_REGISTRY_BYTES;

/// Longest time a deferred status change stays only in memory while the daemon runs.
const FLUSH_INTERVAL: Duration = Duration::from_secs(60);

/// Upper bound for the document envelope around the record array.
const ENVELOPE_BYTES: usize = 128;

type Record = PersistedAgentRuntimeRecord;

/// When a committed mutation must reach the disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Durability {
    /// Replace the file before the mutation returns.
    Immediate,
    /// Mark the document dirty for the next flush.
    Deferred,
}

/// The `agents.json` document cached in memory, with its change time and write state.
#[derive(Debug)]
pub(super) struct AgentsStatus {
    path: PathBuf,
    interval: Duration,
    state: Mutex<State>,
    /// Dropping the sender stops the flusher; `None` means it could not start.
    flusher: OnceLock<Option<Sender<Infallible>>>,
}

#[derive(Debug, Default)]
struct State {
    loaded: bool,
    records: IndexMap<String, Stored>,
    /// Serialized size of all records, kept so oversized changes are rejected before commit.
    bytes: usize,
    snapshot: Option<Arc<[Arc<Record>]>>,
    last_changed_at: Option<String>,
    /// The loaded file's change time when it was not closed cleanly.
    interrupted_since: Option<String>,
    dirty: bool,
    closed: bool,
    /// What the file on disk claims; while it claims `Closed`, the first deferred change writes
    /// at once, so a crash before the next write still triggers recovery.
    on_disk: Marker,
}

/// The `closed` marker of the file on disk.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Marker {
    #[default]
    Open,
    Closed,
}

impl From<bool> for Marker {
    fn from(closed: bool) -> Self {
        if closed { Self::Closed } else { Self::Open }
    }
}

#[derive(Debug, Clone)]
struct Stored {
    record: Arc<Record>,
    bytes: usize,
}

/// Persisted form written by this store.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Document<'a> {
    last_changed_at: Option<&'a str>,
    closed: bool,
    agents: Agents<'a>,
}

/// Serializes the cached records in insertion order without copying them.
struct Agents<'a>(&'a IndexMap<String, Stored>);

impl Serialize for Agents<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.values().map(|stored| stored.record.as_ref()))
    }
}

/// Persisted form read back; legacy files are a bare record array instead.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredDocument {
    last_changed_at: Option<String>,
    #[serde(default)]
    closed: bool,
    agents: Vec<Record>,
}

impl AgentsStatus {
    /// Select `path` without reading it; the file loads on first use.
    pub(super) fn new(path: PathBuf) -> Self {
        Self::with_interval(path, FLUSH_INTERVAL)
    }

    /// Select `path` with a custom flush `interval`.
    pub(super) fn with_interval(path: PathBuf, interval: Duration) -> Self {
        Self {
            path,
            interval,
            state: Mutex::new(State::default()),
            flusher: OnceLock::new(),
        }
    }

    /// Load the file without creating a missing one.
    ///
    /// # Errors
    /// Returns `InvalidFile` for malformed or oversized content and `Io` for read failures.
    pub(super) fn initialize(&self) -> Result<(), Error> {
        drop(self.loaded()?);
        Ok(())
    }

    /// Share the insertion-ordered records; repeated reads reuse one allocation until a change.
    ///
    /// # Errors
    /// Returns load failures.
    pub(super) fn snapshot(&self) -> Result<Arc<[Arc<Record>]>, Error> {
        let mut guard = self.loaded()?;
        let state = &mut *guard;
        let snapshot = state.snapshot.get_or_insert_with(|| {
            state
                .records
                .values()
                .map(|stored| stored.record.clone())
                .collect()
        });
        Ok(snapshot.clone())
    }

    /// Share the record for `id`, or none when it is absent.
    ///
    /// # Errors
    /// Returns load failures.
    pub(super) fn get_shared(&self, id: &str) -> Result<Option<Arc<Record>>, Error> {
        Ok(self
            .loaded()?
            .records
            .get(id)
            .map(|stored| stored.record.clone()))
    }

    /// Return the loaded file's `lastChangedAt` when its writer did not close it. Deferred
    /// changes made after that time may be missing. A cleanly closed, legacy or missing file
    /// reports `None`.
    ///
    /// # Errors
    /// Returns load failures.
    pub(super) fn interrupted_since(&self) -> Result<Option<String>, Error> {
        Ok(self.loaded()?.interrupted_since.clone())
    }

    /// Write `record` if it is new or changed, returning whether it was written.
    ///
    /// # Errors
    /// Returns `InvalidRecord` when the record cannot round-trip or the document would exceed
    /// its size limit, and `Io` when the store is closed or the write fails. On any failure the
    /// cache is left unchanged.
    pub(super) fn upsert(&self, record: &Record) -> Result<bool, Error> {
        let mut state = self.open_state()?;
        self.commit(&mut state, record, Durability::Immediate)
    }

    /// Replace the record for `id` with the result of `transform`, persisting per `durability`.
    ///
    /// # Returns
    /// The stored record and whether it changed, or `None` when `id` is absent.
    ///
    /// # Errors
    /// Returns `transform` errors, `InvalidRecord` for records that cannot round-trip or would
    /// exceed the size limit, and `Io` when the store is closed or an immediate write fails. On
    /// any failure the cache is left unchanged.
    pub(super) fn update(
        self: &Arc<Self>,
        id: &str,
        transform: impl FnOnce(&Record) -> Result<Record, Error>,
        durability: Durability,
    ) -> Result<Option<(Record, bool)>, Error> {
        let mut state = self.open_state()?;
        let Some(current) = state.records.get(id) else {
            return Ok(None);
        };
        let next = transform(&current.record)?;
        let changed = self.commit(&mut state, &next, durability)?;
        drop(state);
        if changed && durability == Durability::Deferred {
            self.schedule();
        }
        Ok(Some((next, changed)))
    }

    /// Remove the record for `id` and write the file before returning.
    ///
    /// # Returns
    /// Whether a record was removed; an absent `id` writes nothing.
    ///
    /// # Errors
    /// Returns `Io` when the store is closed or the write fails; a failed write restores the
    /// record at its position.
    pub(super) fn remove(&self, id: &str) -> Result<bool, Error> {
        let mut state = self.open_state()?;
        let Some((index, key, stored)) = state.records.shift_remove_full(id) else {
            return Ok(false);
        };
        state.bytes -= stored.bytes;
        let previous_change = state.last_changed_at.replace(now());
        state.snapshot = None;
        if let Err(error) = self.write(&mut state) {
            state.bytes += stored.bytes;
            state.records.shift_insert(index, key, stored);
            state.last_changed_at = previous_change;
            return Err(error);
        }
        Ok(true)
    }

    /// Write deferred changes now; a clean or closed document writes nothing.
    ///
    /// # Errors
    /// Returns `Io` when the write fails. The changes stay dirty for the next attempt.
    pub(super) fn flush(&self) -> Result<(), Error> {
        let mut state = self.state.lock().map_err(|_| Error::Io)?;
        if state.dirty && !state.closed {
            self.write(&mut state)?;
        }
        Ok(())
    }

    /// Write a final document marked `closed`, then reject every later mutation and stop the
    /// flusher. A store that was never loaded writes nothing; closing twice is a no-op.
    ///
    /// # Errors
    /// Returns `Io` when the final write fails; the store is closed regardless.
    pub(super) fn close(&self) -> Result<(), Error> {
        let mut state = self.state.lock().map_err(|_| Error::Io)?;
        let first = !state.closed && state.loaded;
        state.closed = true;
        if first {
            self.write(&mut state)?;
        }
        Ok(())
    }

    fn loaded(&self) -> Result<MutexGuard<'_, State>, Error> {
        let mut state = self.state.lock().map_err(|_| Error::Io)?;
        if !state.loaded {
            match crate::File::new(&self.path).read_limited(MAX_REGISTRY_BYTES) {
                Ok(bytes) => load(&mut state, &bytes)?,
                Err(crate::Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(crate::Error::TooLarge) => return Err(Error::InvalidFile),
                Err(crate::Error::Io(_)) => return Err(Error::Io),
            }
            state.loaded = true;
        }
        Ok(state)
    }

    fn open_state(&self) -> Result<MutexGuard<'_, State>, Error> {
        let state = self.loaded()?;
        if state.closed {
            return Err(Error::Io);
        }
        Ok(state)
    }

    /// Install a changed record, then write it or mark it dirty per `durability`.
    fn commit(
        &self,
        state: &mut State,
        record: &Record,
        durability: Durability,
    ) -> Result<bool, Error> {
        let previous = state.records.get(&record.id);
        if previous.is_some_and(|stored| stored.record.as_ref() == record) {
            return Ok(false);
        }
        let bytes = serialized_len(record)?;
        let replaced = previous.map(|stored| stored.bytes);
        let total = state.bytes - replaced.unwrap_or(0) + bytes;
        let count = state.records.len() + usize::from(replaced.is_none());
        if total + count + ENVELOPE_BYTES > max_document_bytes() {
            return Err(Error::InvalidRecord);
        }
        let id = record.id.clone();
        let stored = Stored {
            record: Arc::new(record.clone()),
            bytes,
        };
        let previous = state.records.insert(id.clone(), stored);
        let previous_bytes = std::mem::replace(&mut state.bytes, total);
        let previous_change = state.last_changed_at.replace(now());
        state.snapshot = None;
        match durability {
            Durability::Deferred => {
                state.dirty = true;
                if state.on_disk == Marker::Closed
                    && let Err(error) = self.write(state)
                {
                    tracing::warn!(%error, "Agent status could not reopen its file; retrying later");
                }
            }
            Durability::Immediate => self.write(state).inspect_err(|_| {
                match previous {
                    Some(previous) => {
                        state.records.insert(id, previous);
                    }
                    None => {
                        state.records.pop();
                    }
                }
                state.bytes = previous_bytes;
                state.last_changed_at = previous_change;
            })?,
        }
        Ok(true)
    }

    fn write(&self, state: &mut State) -> Result<(), Error> {
        let document = Document {
            last_changed_at: state.last_changed_at.as_deref(),
            closed: state.closed,
            agents: Agents(&state.records),
        };
        let bytes = serde_json::to_vec(&document).map_err(|_| Error::InvalidRecord)?;
        // Size accounting rejects oversized changes earlier; this keeps a drift from producing a
        // file the next start cannot load.
        if bytes.len() > max_document_bytes() {
            return Err(Error::InvalidRecord);
        }
        crate::File::new(&self.path)
            .write(&bytes)
            .map_err(|_| Error::Io)?;
        state.dirty = false;
        state.on_disk = Marker::from(state.closed);
        Ok(())
    }

    /// Start the flusher on first use; without one, deferred changes are written at once.
    /// A failed fallback write keeps the changes dirty for the next write, so it is only logged.
    fn schedule(self: &Arc<Self>) {
        let running = self
            .flusher
            .get_or_init(|| spawn_flusher(Arc::downgrade(self), self.interval))
            .is_some();
        if !running && let Err(error) = self.flush() {
            tracing::warn!(%error, "Agent status write failed; it will be retried");
        }
    }

    /// Flush for the background thread; returns false once the store is closed.
    fn tick(&self) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if state.closed {
            return false;
        }
        if state.dirty
            && let Err(error) = self.write(&mut state)
        {
            tracing::warn!(%error, "Deferred Agent status flush failed");
        }
        true
    }
}

impl Drop for AgentsStatus {
    fn drop(&mut self) {
        if let Err(error) = self.flush() {
            tracing::error!(%error, "Agent status could not be written on drop");
        }
    }
}

/// Flush `status` every `interval` until it closes or drops; the returned sender keeps the
/// thread waiting.
fn spawn_flusher(status: Weak<AgentsStatus>, interval: Duration) -> Option<Sender<Infallible>> {
    let (stop, stopped) = mpsc::channel();
    std::thread::Builder::new()
        .name("agents-status-flush".to_owned())
        .spawn(move || {
            while let Err(RecvTimeoutError::Timeout) = stopped.recv_timeout(interval) {
                if !status.upgrade().is_some_and(|status| status.tick()) {
                    return;
                }
            }
        })
        .inspect_err(|error| {
            tracing::error!(%error, "Agent status flusher did not start; writing status at once");
        })
        .ok()
        .map(|_| stop)
}

/// Decode a current document or a legacy bare array into `state`.
fn load(state: &mut State, bytes: &[u8]) -> Result<(), Error> {
    let legacy = bytes.iter().find(|byte| !byte.is_ascii_whitespace()) == Some(&b'[');
    let (records, last_changed_at, closed) = if legacy {
        let records = serde_json::from_slice(bytes).map_err(|_| Error::InvalidFile)?;
        (records, None, true)
    } else {
        let document: StoredDocument =
            serde_json::from_slice(bytes).map_err(|_| Error::InvalidFile)?;
        (document.agents, document.last_changed_at, document.closed)
    };
    for record in records {
        let bytes = serialized_len(&record).map_err(|_| Error::InvalidFile)?;
        state.bytes += bytes;
        let stored = Stored {
            record: Arc::new(record),
            bytes,
        };
        if let Some(previous) = state.records.insert(stored.record.id.clone(), stored) {
            state.bytes -= previous.bytes;
        }
    }
    if !closed {
        state.interrupted_since.clone_from(&last_changed_at);
    }
    state.on_disk = Marker::from(closed);
    state.last_changed_at = last_changed_at;
    Ok(())
}

/// Serialized size of `record`, rejecting records that cannot round-trip the persisted schema.
fn serialized_len(record: &Record) -> Result<usize, Error> {
    let bytes = serde_json::to_vec(record).map_err(|_| Error::InvalidRecord)?;
    serde_json::from_slice::<Record>(&bytes).map_err(|_| Error::InvalidRecord)?;
    Ok(bytes.len())
}

/// The shared registry limit as `usize`; a function because the conversion is not `const`.
fn max_document_bytes() -> usize {
    usize::try_from(MAX_REGISTRY_BYTES).unwrap_or(usize::MAX)
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests;
