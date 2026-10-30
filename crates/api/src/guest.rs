//! Guest connections: one visiting account confined to one shared workspace.
//!
//! The hub bridge dials the local API with the daemon's own bearer credential and a
//! [`model::share::GUEST_HEADER`] naming the visitor. Such a connection may use Agents,
//! terminals and files of exactly one workspace; every other method is refused.
//!
//! This limits which RPCs a guest can reach; it is not a sandbox. A guest with the
//! `operate` permission runs arbitrary commands as the Host user through terminals and
//! Agents in the shared workspace, and those processes are not confined by this policy.

pub(crate) mod policy;

use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use axum::http::StatusCode;
use model::ErrorCode;
use model::share::Visitor;
use tokio_util::sync::CancellationToken;

use crate::Shared;
use crate::shares::guests::Registration;

/// Outstanding download tokens one guest connection may hold.
const MAX_DOWNLOADS: usize = 16;

/// Lifetime of a download token, matching the file service.
const DOWNLOAD_TTL: Duration = Duration::from_secs(60);

/// Verified terminal identities cached per connection; terminals never change workspace.
const MAX_KNOWN_TERMINALS: usize = 128;

/// Why a guest upgrade is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum AdmissionError {
    /// The guest header is not a valid visitor.
    #[error("malformed guest visitor")]
    Malformed,
    /// No accepted share admits this visitor to an active workspace.
    #[error("guest is not admitted")]
    Forbidden,
    /// The workspace directory could not be read.
    #[error("workspace directory unavailable")]
    Unavailable,
}

impl From<AdmissionError> for StatusCode {
    fn from(error: AdmissionError) -> Self {
        match error {
            AdmissionError::Malformed => Self::BAD_REQUEST,
            AdmissionError::Forbidden => Self::FORBIDDEN,
            AdmissionError::Unavailable => Self::SERVICE_UNAVAILABLE,
        }
    }
}

/// Scope and per-connection state of one admitted guest connection.
#[derive(Debug)]
pub(crate) struct Session {
    visitor: Visitor,
    root: PathBuf,
    root_text: String,
    downloads: Mutex<VecDeque<Instant>>,
    terminals: Mutex<BTreeSet<String>>,
    registration: Registration,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Admit the guest named by a [`model::share::GUEST_HEADER`] value.
///
/// The workspace must exist and be active, and an accepted share must admit the visitor.
///
/// # Errors
/// Returns [`AdmissionError`] for malformed headers, refused visitors, or directory failures.
pub(crate) async fn admit(state: &Shared, header: &str) -> Result<Session, AdmissionError> {
    let visitor = Visitor::decode(header).ok_or(AdmissionError::Malformed)?;
    let workspace = crate::shares::rpc::active_workspace(state, &visitor.workspace_key)
        .await
        .map_err(|_| AdmissionError::Unavailable)?
        .ok_or(AdmissionError::Forbidden)?;
    let root = tokio::fs::canonicalize(&workspace.cwd)
        .await
        .map_err(|_| AdmissionError::Forbidden)?;
    let registration = state
        .shares
        .admit(&visitor)
        .ok_or(AdmissionError::Forbidden)?;
    Ok(Session::new(visitor, root, registration))
}

impl Session {
    fn new(visitor: Visitor, root: PathBuf, registration: Registration) -> Self {
        Self {
            root_text: root.to_string_lossy().into_owned(),
            visitor,
            root,
            downloads: Mutex::new(VecDeque::new()),
            terminals: Mutex::new(BTreeSet::new()),
            registration,
        }
    }

    /// Cancelled when the share stops admitting this guest.
    pub(crate) fn cancellation(&self) -> &CancellationToken {
        self.registration.cancellation()
    }

    /// The only workspace this connection may use.
    pub(crate) fn workspace_id(&self) -> &str {
        &self.visitor.workspace_key
    }

    /// Whether `cwd` is an absolute path that resolves inside the workspace root,
    /// following symbolic links and `..` components.
    pub(crate) async fn contains(&self, cwd: &str) -> bool {
        let path = Path::new(cwd);
        if !path.is_absolute() {
            return false;
        }
        tokio::fs::canonicalize(path)
            .await
            .is_ok_and(|resolved| resolved.starts_with(&self.root))
    }

    /// Reserve one download token, refusing more than [`MAX_DOWNLOADS`] unexpired tokens.
    fn take_download(&self, now: Instant) -> bool {
        let mut downloads = lock(&self.downloads);
        while downloads
            .front()
            .is_some_and(|issued| now.duration_since(*issued) >= DOWNLOAD_TTL)
        {
            downloads.pop_front();
        }
        if downloads.len() >= MAX_DOWNLOADS {
            return false;
        }
        downloads.push_back(now);
        true
    }

    fn knows_terminal(&self, id: &str) -> bool {
        lock(&self.terminals).contains(id)
    }

    fn remember_terminal(&self, id: &str) {
        let mut terminals = lock(&self.terminals);
        if terminals.len() >= MAX_KNOWN_TERMINALS {
            terminals.clear();
        }
        terminals.insert(id.to_owned());
    }
}

/// Map a lookup failure to a reply that never discloses resources outside the workspace.
fn conceal(code: ErrorCode) -> ErrorCode {
    match code {
        ErrorCode::ResourceExhausted | ErrorCode::ServerDraining => code,
        _ => ErrorCode::Forbidden,
    }
}

#[cfg(test)]
mod tests;
