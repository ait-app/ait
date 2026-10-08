//! Generic, insertion-ordered atomic JSON registries.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use indexmap::IndexMap;
use serde::{Serialize, de::DeserializeOwned};

/// A registry load, validation, or commit failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// A staged record cannot round-trip through its persisted schema.
    #[error("invalid registry record")]
    InvalidRecord,
    /// Existing bytes do not represent the required JSON array.
    #[error("invalid registry file")]
    InvalidFile,
    /// Reading or atomically replacing the file failed.
    #[error("registry I/O failed")]
    Io,
    /// Mutations are frozen until the registry instance is replaced.
    #[error("registry mutations are blocked until restart")]
    Frozen,
}

/// Atomic writer supplied by a host; it must install all bytes or report failure.
pub type Writer<E = Error> = Arc<dyn Fn(&Path, &[u8]) -> Result<(), E> + Send + Sync>;

/// Shared atomic JSON array engine; business registries validate and publish their own records.
/// Hosts must serialize independent writers with a data-directory lease.
///
/// # Examples
///
/// ```
/// use persistence::registry::FileRegistry;
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Clone, Deserialize, Serialize)]
/// struct Record { id: String }
///
/// let directory = tempfile::tempdir()?;
/// let registry: FileRegistry<Record> =
///     FileRegistry::new(directory.path().join("records.json"), |record| &record.id);
/// registry.mutate(|records| {
///     let record = Record { id: "first".to_owned() };
///     records.insert(record.id.clone(), record);
///     Ok(((), true))
/// })?;
/// assert!(registry.get("first")?.is_some());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct FileRegistry<R, E = Error> {
    path: PathBuf,
    state: Mutex<State<R>>,
    writer: Writer<E>,
    id: fn(&R) -> &str,
}

struct State<R> {
    loaded: bool,
    frozen: bool,
    records: IndexMap<String, R>,
}

impl<R, E> fmt::Debug for FileRegistry<R, E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileRegistry")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl<R: Clone + Serialize + DeserializeOwned, E: From<Error> + 'static> FileRegistry<R, E> {
    /// Create a lazy registry at `path`, keyed by the identity returned by `id`.
    #[must_use]
    pub fn new(path: PathBuf, id: fn(&R) -> &str) -> Self {
        Self {
            path,
            state: Mutex::new(State {
                loaded: false,
                frozen: false,
                records: IndexMap::new(),
            }),
            writer: Arc::new(write_atomic),
            id,
        }
    }

    /// Clone the atomic writer so a host can decorate it with transaction instrumentation.
    #[must_use]
    pub fn writer(&self) -> Writer<E> {
        self.writer.clone()
    }

    /// Replace the writer with `writer` before sharing the registry with other owners.
    /// The caller must preserve atomic installation and its existing commit semantics.
    pub fn set_writer(&mut self, writer: Writer<E>) {
        self.writer = writer;
    }

    fn loaded(&self) -> Result<MutexGuard<'_, State<R>>, E> {
        let mut state = self.state.lock().map_err(|_| E::from(Error::Frozen))?;
        if !state.loaded {
            let records: Vec<R> = match crate::File::new(self.path.clone()).read() {
                Ok(bytes) => {
                    serde_json::from_slice(&bytes).map_err(|_| E::from(Error::InvalidFile))?
                }
                Err(crate::Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                    Vec::new()
                }
                Err(_) => return Err(E::from(Error::Io)),
            };
            for record in records {
                state.records.insert((self.id)(&record).to_owned(), record);
            }
            state.loaded = true;
        }
        Ok(state)
    }

    /// Load the JSON array without creating a missing file.
    ///
    /// # Errors
    /// Returns malformed-file, lock or filesystem errors.
    pub fn initialize(&self) -> Result<(), E> {
        drop(self.loaded()?);
        Ok(())
    }

    /// Return whether the selected path exists; inaccessible paths report false.
    #[must_use]
    pub fn exists(&self) -> bool {
        self.path.try_exists().unwrap_or(false)
    }

    /// Read committed records in insertion order.
    ///
    /// # Errors
    /// Returns malformed-file, lock or filesystem errors.
    pub fn list(&self) -> Result<Vec<R>, E> {
        Ok(self.loaded()?.records.values().cloned().collect())
    }

    /// Read the committed record for `id`, or none when it is absent.
    ///
    /// # Errors
    /// Returns malformed-file, lock or filesystem errors.
    pub fn get(&self, id: &str) -> Result<Option<R>, E> {
        Ok(self.loaded()?.records.get(id).cloned())
    }

    /// Reject future mutations on this instance while keeping committed reads available.
    /// # Errors
    /// Returns loading or poisoned-lock errors.
    pub fn freeze(&self) -> Result<(), E> {
        self.loaded()?.frozen = true;
        Ok(())
    }

    // The bool is intentional: Paseo upsert/update writes even for structurally equal
    // records, while absent removals and already-archived projects are true no-ops.
    /// Transform a staged record map and atomically publish it when `update` returns true.
    ///
    /// The callback returns its result and whether the file must be replaced.
    ///
    /// # Errors
    /// Returns callback, validation, frozen-registry, lock or file-write errors.
    pub fn mutate<T>(
        &self,
        update: impl FnOnce(&mut IndexMap<String, R>) -> Result<(T, bool), E>,
    ) -> Result<T, E> {
        self.mutate_with(update, |_| Ok(()), || Ok(()))
    }

    /// Commit a staged change with hooks under the same registry lock.
    /// `before_write` receives the committed before-image; `after_write` runs after installation.
    /// Failed hooks leave the cache unchanged; the caller owns journal recovery after a disk write.
    /// # Errors
    /// Returns loading, validation, frozen-state, writer, or hook errors.
    pub fn mutate_with<T>(
        &self,
        update: impl FnOnce(&mut IndexMap<String, R>) -> Result<(T, bool), E>,
        before_write: impl FnOnce(&[R]) -> Result<(), E>,
        after_write: impl FnOnce() -> Result<(), E>,
    ) -> Result<T, E> {
        let mut state = self.loaded()?;
        if state.frozen {
            return Err(E::from(Error::Frozen));
        }
        let mut staged = state.records.clone();
        let (result, changed) = update(&mut staged)?;
        if changed {
            let records: Vec<&R> = staged.values().collect();
            let bytes =
                serde_json::to_vec_pretty(&records).map_err(|_| E::from(Error::InvalidRecord))?;
            // Validate programmatically constructed records too (e.g. positive request numbers).
            serde_json::from_slice::<Vec<R>>(&bytes).map_err(|_| E::from(Error::InvalidRecord))?;
            // Journal before-images must come from the same locked state as the write.
            let records = state.records.values().cloned().collect::<Vec<_>>();
            before_write(&records)?;
            (self.writer)(&self.path, &bytes)?;
            after_write()?;
            state.records = staged;
        }
        Ok(result)
    }
}

fn write_atomic<E: From<Error>>(path: &Path, bytes: &[u8]) -> Result<(), E> {
    crate::File::new(path)
        .write(bytes)
        .map_err(|_| E::from(Error::Io))
}

#[cfg(test)]
mod tests;
