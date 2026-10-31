//! Guest connections: one visiting account confined to one shared resource.
//!
//! The hub bridge dials the local API with the daemon's own bearer credential and a
//! [`model::share::GUEST_HEADER`] naming the visitor. A share covers one workspace, one
//! project with all its workspaces, or the whole node (Host). Within that scope such a
//! connection may use Agents, terminals and files; a whole-node grant also manages
//! resources, configuration and daemon lifecycle, while account authority stays private.
//!
//! This limits which RPCs a guest can reach; it is not a sandbox. A guest with the
//! `operate` permission runs arbitrary commands as the Host user through terminals and
//! Agents in the shared scope, and those processes are not confined by this policy.

pub(crate) mod policy;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use axum::http::StatusCode;
use domain::workspace::records::PersistedWorkspaceRecord;
use model::ErrorCode;
use model::share::{ResourceKind, ShareScope, Visitor};
use tokio_util::sync::CancellationToken;

use crate::Shared;
use crate::shares::guests::Registration;
use crate::shares::rpc::{active_project, active_workspace};

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
    /// No accepted share admits this visitor to an active resource.
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

/// What an admitted guest connection may reach, resolved at admission.
#[derive(Debug)]
enum Reach {
    /// One workspace and its canonical directory.
    Workspace {
        id: String,
        root: PathBuf,
        root_text: String,
    },
    /// One project, its canonical root, and the workspaces of the project seen so far.
    Project {
        id: String,
        root: PathBuf,
        known: Mutex<ProjectWorkspaces>,
    },
    /// Every project and workspace on this Host.
    Node,
}

/// Workspaces of a shared project. Workspaces never change project, so entries stay valid;
/// a miss reloads the directory to pick up workspaces created since.
#[derive(Debug, Default)]
struct ProjectWorkspaces {
    /// Workspace ID to registered directory, archived workspaces included.
    directories: BTreeMap<String, String>,
    /// Canonical directories of the active workspaces.
    roots: Vec<PathBuf>,
}

/// Scope and per-connection state of one admitted guest connection.
#[derive(Debug)]
pub(crate) struct Session {
    reach: Reach,
    downloads: Mutex<VecDeque<Instant>>,
    terminals: Mutex<BTreeSet<String>>,
    registration: Registration,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Admit the guest named by a [`model::share::GUEST_HEADER`] value.
///
/// A shared workspace or project must exist and be active, a node share must carry an
/// empty key, and an accepted share must admit the visitor.
///
/// # Errors
/// Returns [`AdmissionError`] for malformed headers, refused visitors, or directory failures.
pub(crate) async fn admit(state: &Shared, header: &str) -> Result<Session, AdmissionError> {
    let visitor = Visitor::decode(header).ok_or(AdmissionError::Malformed)?;
    let scope = visitor.scope().ok_or(AdmissionError::Forbidden)?;
    let reach = resolve(state, scope).await?;
    let registration = state
        .shares
        .admit(&visitor)
        .ok_or(AdmissionError::Forbidden)?;
    Ok(Session::new(reach, registration))
}

/// Resolve the directories a scope reaches, refusing inactive workspaces and projects.
async fn resolve(state: &Shared, scope: ShareScope) -> Result<Reach, AdmissionError> {
    match scope {
        ShareScope::Workspace(id) => {
            let workspace = active_workspace(state, &id)
                .await
                .map_err(|_| AdmissionError::Unavailable)?
                .ok_or(AdmissionError::Forbidden)?;
            let root = canonical(&workspace.cwd).await?;
            Ok(Reach::Workspace {
                id,
                root_text: root.to_string_lossy().into_owned(),
                root,
            })
        }
        ShareScope::Project(id) => {
            let project = active_project(state, &id)
                .await
                .map_err(|_| AdmissionError::Unavailable)?
                .ok_or(AdmissionError::Forbidden)?;
            Ok(Reach::Project {
                id,
                root: canonical(&project.root_path).await?,
                known: Mutex::default(),
            })
        }
        ShareScope::Node => Ok(Reach::Node),
    }
}

async fn canonical(path: &str) -> Result<PathBuf, AdmissionError> {
    tokio::fs::canonicalize(path)
        .await
        .map_err(|_| AdmissionError::Forbidden)
}

/// Every workspace of project `id`, archived ones included.
///
/// # Errors
/// Returns `UnsupportedCapability` without a directory, or registry failures.
async fn project_workspaces(
    state: &Shared,
    id: &str,
) -> Result<Vec<PersistedWorkspaceRecord>, ErrorCode> {
    let id = id.to_owned();
    state
        .runtime
        .run_queued(
            state.metadata.directory.clone(),
            ErrorCode::RegistryIo,
            move |directory| {
                let workspaces = directory
                    .list_workspaces()
                    .map_err(|_| ErrorCode::RegistryIo)?;
                Ok(workspaces
                    .into_iter()
                    .filter(|workspace| workspace.project_id == id)
                    .collect())
            },
        )
        .await
}

fn active(workspace: &PersistedWorkspaceRecord) -> bool {
    workspace.archived_at.as_ref().is_none_or(String::is_empty)
}

impl Session {
    fn new(reach: Reach, registration: Registration) -> Self {
        Self {
            reach,
            downloads: Mutex::new(VecDeque::new()),
            terminals: Mutex::new(BTreeSet::new()),
            registration,
        }
    }

    /// Cancelled when the share stops admitting this guest.
    pub(crate) fn cancellation(&self) -> &CancellationToken {
        self.registration.cancellation()
    }

    /// Kind of the shared resource.
    pub(crate) fn kind(&self) -> ResourceKind {
        match self.reach {
            Reach::Workspace { .. } => ResourceKind::Workspace,
            Reach::Project { .. } => ResourceKind::Project,
            Reach::Node => ResourceKind::Node,
        }
    }

    /// The shared project, for project shares.
    fn project_id(&self) -> Option<&str> {
        match &self.reach {
            Reach::Project { id, .. } => Some(id),
            Reach::Workspace { .. } | Reach::Node => None,
        }
    }

    /// Whether `cwd` is inside the shared scope. Workspace and project shares require an
    /// absolute path that resolves, following symbolic links and `..` components, inside the
    /// workspace root, or inside the project root or one of the project's active workspace
    /// directories. A node share reaches every path.
    ///
    /// # Errors
    /// Returns directory failures while reloading a project's workspaces.
    pub(crate) async fn contains(&self, state: &Shared, cwd: &str) -> Result<bool, ErrorCode> {
        let (root, known) = match &self.reach {
            Reach::Node => return Ok(true),
            Reach::Workspace { root, .. } => (root, None),
            Reach::Project { root, known, .. } => (root, Some(known)),
        };
        let path = Path::new(cwd);
        if !path.is_absolute() {
            return Ok(false);
        }
        let Ok(resolved) = tokio::fs::canonicalize(path).await else {
            return Ok(false);
        };
        if resolved.starts_with(root) {
            return Ok(true);
        }
        let Some(known) = known else {
            return Ok(false);
        };
        let inside = |known: &ProjectWorkspaces| {
            known
                .roots
                .iter()
                .any(|workspace| resolved.starts_with(workspace))
        };
        if inside(&lock(known)) {
            return Ok(true);
        }
        self.reload(state).await?;
        Ok(inside(&lock(known)))
    }

    /// Whether workspace `id` is inside the shared scope; every workspace is for a node.
    ///
    /// # Errors
    /// Returns directory failures while reloading a project's workspaces.
    async fn reaches(&self, state: &Shared, id: &str) -> Result<bool, ErrorCode> {
        if matches!(self.reach, Reach::Node) {
            return Ok(true);
        }
        Ok(self.directory(state, id).await?.is_some())
    }

    /// Registered directory of workspace `id` when a workspace or project share covers it.
    /// A node share answers `None`: its guests place resources themselves.
    ///
    /// # Errors
    /// Returns directory failures while reloading a project's workspaces.
    async fn directory(&self, state: &Shared, id: &str) -> Result<Option<String>, ErrorCode> {
        match &self.reach {
            Reach::Workspace {
                id: shared,
                root_text,
                ..
            } => Ok((shared == id).then(|| root_text.clone())),
            Reach::Project { known, .. } => {
                if let Some(directory) = lock(known).directories.get(id).cloned() {
                    return Ok(Some(directory));
                }
                self.reload(state).await?;
                Ok(lock(known).directories.get(id).cloned())
            }
            Reach::Node => Ok(None),
        }
    }

    /// Whether `cwd` may be opened as a workspace. A project share admits only the directory
    /// of one of its active workspaces: opening any other directory could register a new
    /// project outside the share. A node share opens every directory; a workspace share none.
    ///
    /// # Errors
    /// Returns directory failures while reloading a project's workspaces.
    async fn opens(&self, state: &Shared, cwd: &str) -> Result<bool, ErrorCode> {
        let known = match &self.reach {
            Reach::Node => return Ok(true),
            Reach::Workspace { .. } => return Ok(false),
            Reach::Project { known, .. } => known,
        };
        let path = Path::new(cwd);
        if !path.is_absolute() {
            return Ok(false);
        }
        let Ok(resolved) = tokio::fs::canonicalize(path).await else {
            return Ok(false);
        };
        if lock(known).roots.contains(&resolved) {
            return Ok(true);
        }
        self.reload(state).await?;
        Ok(lock(known).roots.contains(&resolved))
    }

    /// Reload the workspaces of a shared project; other scopes have nothing to reload.
    async fn reload(&self, state: &Shared) -> Result<(), ErrorCode> {
        let Reach::Project { id, known, .. } = &self.reach else {
            return Ok(());
        };
        let workspaces = project_workspaces(state, id).await?;
        let mut roots = Vec::with_capacity(workspaces.len());
        for workspace in workspaces.iter().filter(|workspace| active(workspace)) {
            if let Ok(root) = tokio::fs::canonicalize(&workspace.cwd).await {
                roots.push(root);
            }
        }
        let directories = workspaces
            .into_iter()
            .map(|workspace| (workspace.workspace_id, workspace.cwd))
            .collect();
        *lock(known) = ProjectWorkspaces { directories, roots };
        Ok(())
    }

    /// IDs of every workspace in a shared project, archived ones included; none for
    /// other scopes.
    ///
    /// # Errors
    /// Returns directory failures.
    async fn project_workspace_ids(&self, state: &Shared) -> Result<Vec<String>, ErrorCode> {
        let Some(project) = self.project_id() else {
            return Ok(Vec::new());
        };
        Ok(project_workspaces(state, project)
            .await?
            .into_iter()
            .map(|workspace| workspace.workspace_id)
            .collect())
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

    /// Whether this visitor created terminal `id`; only those take its input.
    pub(crate) fn owns_terminal(&self, id: &str) -> bool {
        self.registration.owns_terminal(id)
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

/// Map a lookup failure to a reply that never discloses resources outside the shared scope.
fn conceal(code: ErrorCode) -> ErrorCode {
    match code {
        ErrorCode::ResourceExhausted | ErrorCode::ServerDraining => code,
        _ => ErrorCode::Forbidden,
    }
}

#[cfg(test)]
mod tests;
