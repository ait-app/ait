//! Latest-state directory sequencing, shared by capability-owned projections.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use domain::directory_sync::{Cursor, Metadata, Mode, Read, Reason, Removal};
use serde_json::Value;

/// Shared generation with independent sequences for each capability-owned collection.
#[derive(Debug, Clone)]
pub struct DirectorySync {
    generation: Arc<str>,
    collections: Arc<Mutex<BTreeMap<&'static str, Collection>>>,
}

#[derive(Debug, Default)]
struct Collection {
    values: BTreeMap<String, Versioned>,
    removals: BTreeMap<u64, String>,
    removed_ids: BTreeMap<String, u64>,
    head: u64,
    expired_through: u64,
}

#[derive(Debug)]
struct Versioned {
    seq: u64,
    value: Value,
}

impl DirectorySync {
    /// Create empty collections for a host-generated process identity.
    #[must_use]
    pub fn new(generation: String) -> Self {
        Self {
            generation: generation.into(),
            collections: Arc::default(),
        }
    }

    /// Atomically reconcile a complete projection and read from the client's checkpoint.
    ///
    /// `collection` is a stable name owned by the caller. Rows must be JSON objects with unique
    /// identities and must exclude transport-only sequence fields. Unchanged rows retain their
    /// sequence. The most recent 5,000 removals remain available for reconnecting clients.
    #[must_use]
    pub fn synchronize(
        &self,
        collection: &'static str,
        rows: impl IntoIterator<Item = (String, Value)>,
        cursor: &Cursor,
    ) -> Read {
        let mut collections = self
            .collections
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let collection = collections.entry(collection).or_default();
        collection.replace_all(rows, 5_000);
        collection.read(&self.generation, cursor)
    }
}

impl Collection {
    fn replace_all(&mut self, rows: impl IntoIterator<Item = (String, Value)>, limit: usize) {
        let mut retained = BTreeSet::new();
        for (id, value) in rows {
            retained.insert(id.clone());
            if self.values.get(&id).is_some_and(|old| old.value == value) {
                continue;
            }
            if let Some(seq) = self.removed_ids.remove(&id) {
                self.removals.remove(&seq);
            }
            self.head += 1;
            self.values.insert(
                id,
                Versioned {
                    seq: self.head,
                    value,
                },
            );
        }
        self.values.retain(|id, _| {
            if retained.contains(id) {
                return true;
            }
            self.head += 1;
            self.removals.insert(self.head, id.clone());
            self.removed_ids.insert(id.clone(), self.head);
            false
        });
        while self.removals.len() > limit {
            if let Some((seq, id)) = self.removals.pop_first() {
                self.removed_ids.remove(&id);
                self.expired_through = seq;
            }
        }
    }

    fn read(&self, generation: &str, cursor: &Cursor) -> Read {
        let reason = match (cursor.generation.as_deref(), cursor.after_seq) {
            (Some(previous), _) if previous != generation => Some(Reason::GenerationChanged),
            (None, _) | (_, None) => Some(Reason::NoCursor),
            (_, Some(seq)) if seq < self.expired_through || seq > self.head => {
                Some(Reason::CursorExpired)
            }
            (_, Some(_)) => None,
        };
        let after = if reason.is_some() {
            0
        } else {
            cursor.after_seq.unwrap_or(0)
        };
        let mut values: Vec<_> = self.values.values().filter(|row| row.seq > after).collect();
        values.sort_unstable_by_key(|row| row.seq);
        Read {
            values: values
                .into_iter()
                .map(|row| {
                    let mut value = row.value.clone();
                    value["syncSeq"] = row.seq.into();
                    value
                })
                .collect(),
            sync: Metadata {
                generation: generation.to_owned(),
                head_seq: self.head,
                mode: if reason.is_some() {
                    Mode::Snapshot
                } else {
                    Mode::Changes
                },
                reason,
                removals: if reason.is_some() {
                    Vec::new()
                } else {
                    self.removals
                        .range((std::ops::Bound::Excluded(after), std::ops::Bound::Unbounded))
                        .map(|(&seq, id)| Removal {
                            id: id.clone(),
                            seq,
                        })
                        .collect()
                },
            },
        }
    }
}

#[cfg(test)]
mod tests;
