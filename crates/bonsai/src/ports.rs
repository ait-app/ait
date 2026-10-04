//! Host boundaries: Agent execution, live Agent events, timeline backfill and projects.
//!
//! The adapter names no provider, metadata or filesystem types; the host (`daemon`)
//! implements these ports over `AgentExecution`, `Timeline` and `Directory`.

use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;

use model::ErrorCode;
use serde_json::Value;
use tokio::sync::mpsc;

/// Boxed `Send` future returned by asynchronous port methods.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Host failures that carry no paths, credentials or provider detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PortError {
    /// Host storage or a blocking task failed.
    #[error("host storage failed")]
    Storage,
    /// The observation could not be activated or has closed.
    #[error("agent observation closed")]
    Closed,
}

/// In-process Agent RPC (`AgentExecution::execute`).
pub trait Executor: Send + Sync + Debug {
    /// Execute one Agent or provider method.
    ///
    /// # Arguments
    ///
    /// * `method` - Request method such as `agent.create.request`.
    /// * `params` - Request parameters.
    ///
    /// # Errors
    ///
    /// Returns the host's stable error code; `CatalogBusy` is retryable.
    fn execute(
        &self,
        method: &'static str,
        params: Value,
    ) -> BoxFuture<'_, Result<Value, ErrorCode>>;
}

/// One live Agent event: the method and params of AIT's `ServerMessage::Event`.
#[derive(Debug, Clone, PartialEq)]
pub struct HostEvent {
    /// Event method, such as `agent_stream`.
    pub method: String,
    /// Event params.
    pub params: Value,
}

/// A paused observation of one Agent's live events.
///
/// Events published before [`Observation::activate`] are buffered by the host and delivered
/// first. [`Observation::next`] returns `None` once the host closed the observation (overflow
/// or shutdown); the adapter then re-observes and backfills.
pub struct Observation {
    events: mpsc::UnboundedReceiver<HostEvent>,
    activate: Option<Box<dyn FnOnce() -> Result<(), PortError> + Send + Sync>>,
}

impl Observation {
    /// Wrap a host event channel and its activation step.
    ///
    /// # Arguments
    ///
    /// * `events` - Channel the host forwards events into; closing it ends the observation.
    /// * `activate` - Starts delivery of buffered and later events.
    #[must_use]
    pub fn new(
        events: mpsc::UnboundedReceiver<HostEvent>,
        activate: Box<dyn FnOnce() -> Result<(), PortError> + Send + Sync>,
    ) -> Self {
        Self {
            events,
            activate: Some(activate),
        }
    }

    /// Start delivery; later calls do nothing.
    ///
    /// # Errors
    ///
    /// Returns [`PortError::Closed`] when the host already closed the observation.
    pub fn activate(&mut self) -> Result<(), PortError> {
        self.activate.take().map_or(Ok(()), |activate| activate())
    }

    /// Wait for the next event; `None` once the observation has closed.
    pub async fn next(&mut self) -> Option<HostEvent> {
        self.events.recv().await
    }
}

impl Debug for Observation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Observation")
            .field("active", &self.activate.is_none())
            .finish_non_exhaustive()
    }
}

/// Live Agent events (`Timeline::events()`), keyed by exact Agent ID.
pub trait Observer: Send + Sync + Debug {
    /// Register a paused observation for one Agent.
    ///
    /// # Errors
    ///
    /// Returns [`PortError`] when the host cannot register the observer.
    fn observe(&self, agent_id: &str) -> Result<Observation, PortError>;
}

/// One unmerged timeline row, shaped like the `timeline` event of a live `agent_stream`.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// Sequence number inside the generation.
    pub seq: u64,
    /// Provider that produced the row.
    pub provider: String,
    /// Turn ID, when the provider records one.
    pub turn_id: Option<String>,
    /// Timeline item, identical to the live event's `item`.
    pub item: Value,
}

/// An Agent's current timeline generation (`Timeline::read`): progress deltas and completed
/// suffixes in sequence order, never the merged display projection.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Backlog {
    /// Generation identity.
    pub epoch: String,
    /// Rows in sequence order.
    pub rows: Vec<Row>,
}

/// Timeline reads used to rebuild what live delivery missed.
pub trait Backfill: Send + Sync + Debug {
    /// Read the current generation of an Agent's timeline.
    ///
    /// # Errors
    ///
    /// Returns [`PortError::Storage`] when the read fails.
    fn read(&self, agent_id: &str) -> BoxFuture<'_, Result<Backlog, PortError>>;
}

/// A registered, unarchived project. `root` stays on this machine and is never announced.
#[derive(Clone, PartialEq, Eq)]
pub struct Project {
    /// Host project ID.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Local root directory.
    pub root: String,
    /// `origin` remote URL as Git reports it.
    pub remote_url: Option<String>,
    /// Current branch.
    pub branch: Option<String>,
}

impl Debug for Project {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Project")
            .field("id", &self.id)
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// A workspace opened for a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    /// Host workspace ID.
    pub workspace_id: String,
    /// Directory the Agent runs in.
    pub cwd: String,
}

/// Registered projects and their workspaces.
pub trait Projects: Send + Sync + Debug {
    /// List unarchived projects with their Git remote and branch.
    ///
    /// # Errors
    ///
    /// Returns [`PortError::Storage`] when the registry cannot be read.
    fn list(&self) -> BoxFuture<'_, Result<Vec<Project>, PortError>>;

    /// Reuse, restore or create the workspace for a project's root.
    ///
    /// # Returns
    ///
    /// `None` when the project is unknown, archived or its directory is gone.
    ///
    /// # Errors
    ///
    /// Returns [`PortError::Storage`] for other registry failures.
    fn open_workspace(
        &self,
        project_id: &str,
    ) -> BoxFuture<'_, Result<Option<Workspace>, PortError>>;
}

/// The four host ports the adapter runs on.
#[derive(Debug, Clone)]
pub struct Host {
    /// Agent RPC.
    pub executor: std::sync::Arc<dyn Executor>,
    /// Live events.
    pub observer: std::sync::Arc<dyn Observer>,
    /// Timeline reads.
    pub backfill: std::sync::Arc<dyn Backfill>,
    /// Projects.
    pub projects: std::sync::Arc<dyn Projects>,
}

#[cfg(test)]
mod tests;
