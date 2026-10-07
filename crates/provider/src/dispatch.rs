//! Concrete service state and crate-owned request dispatch.

use std::sync::{Arc, Mutex};

use model::{Context, DispatchError, ErrorCode, Runtime};

mod requests;

/// Services installed for this capability crate, sharing server-wide runtime resources.
#[derive(Debug)]
pub struct State {
    /// Shared Tokio admission, cancellation and task tracking.
    pub runtime: Arc<Runtime>,
    /// Installed agents service.
    pub agents: Option<Arc<Mutex<crate::service::agents::Agents>>>,
    /// Installed agent runtime service.
    pub agent_runtime: Option<Arc<Mutex<crate::service::agent_runtime::AgentRuntimeDirectory>>>,
    /// Installed native Agent executor.
    pub agent_execution: Option<crate::service::agent_execution::AgentExecution>,
    /// Shared wakeup signal for committed Agent and placement changes.
    pub directory_changes: Option<model::changes::Changes>,
    /// Whether the host can finish a coordinated Terminal close.
    pub has_terminals: bool,
}

impl std::ops::Deref for State {
    type Target = Runtime;

    fn deref(&self) -> &Runtime {
        &self.runtime
    }
}

pub(crate) mod agent_execution;
mod agent_runtime;
mod catalog;

pub(crate) use catalog::METHODS as CATALOG_METHODS;

/// Check exact Agent resource occupancy without serializing behind a native startup.
/// # Errors
/// Returns unavailable-service, admission, or registry failures.
pub async fn contains_identity(state: &State, id: String) -> Result<bool, ErrorCode> {
    if let Some(execution) = state.agent_execution.clone() {
        return state
            .runtime
            .run_queued(
                Some(Arc::new(Mutex::new(execution))),
                ErrorCode::AgentIo,
                move |execution| execution.contains_identity(&id).map_err(Into::into),
            )
            .await;
    }
    state
        .runtime
        .run_queued(
            state.agent_runtime.clone(),
            ErrorCode::AgentIo,
            move |directory| {
                directory
                    .contains_identity(&id)
                    .map_err(|_| ErrorCode::AgentIo)
            },
        )
        .await
}

/// Archive and close native Agents after their owning Workspace records become inactive.
/// # Errors
/// Returns registry, worker, or native cleanup failures; the caller must retain checkout files.
pub async fn retire_workspaces(
    state: &State,
    workspace_ids: Vec<String>,
) -> Result<Vec<String>, ErrorCode> {
    if let Some(execution) = &state.agent_execution {
        let value = execution
            .execute(
                "internal.workspace.retire",
                serde_json::json!(workspace_ids),
            )
            .await?;
        return serde_json::from_value(value).map_err(|_| ErrorCode::AgentIo);
    }
    if state.agent_runtime.is_none() {
        return Ok(Vec::new());
    }
    state
        .runtime
        .run_queued(
            state.agent_runtime.clone(),
            ErrorCode::AgentIo,
            move |directory| {
                directory
                    .archive_workspaces(&workspace_ids, &chrono::Utc::now().to_rfc3339())
                    .map_err(|_| ErrorCode::AgentIo)
            },
        )
        .await
}

/// Remaining composition work after provider dispatch has completed.
pub enum Completion {
    /// Close requested Terminals after Agent closure, then send the combined response.
    CloseTerminals {
        /// Correlation ID of the original request.
        request_id: String,
        /// Provider's completed portion of the response.
        value: serde_json::Value,
        /// Terminals to close using the terminal crate.
        terminal_ids: Vec<String>,
    },
}

/// Dispatch an admitted provider request using concrete shared request resources.
/// Leaves `context` unchanged for other crates; takes it when this crate handles the method.
/// Returns `DispatchError::NotImplemented` while leaving an unmatched Context available.
/// Returns optional work for the API to finish before ending request processing.
///
/// # Arguments
/// * `context` - Pending request, consumed only when this crate recognizes its method.
/// * `state` - Installed services and resources used to execute the request.
/// * `connection` - Connection-owned subscriptions and streams for this capability.
///
/// # Errors
/// Returns `NotImplemented` for an unmatched method and `Delivery` for an outbound failure.
/// Business failures are sent using the original request ID.
pub async fn dispatch(
    context: &mut Option<Context<'_>>,
    state: &State,
    connection: &mut crate::connection::Connection,
) -> Result<Option<Completion>, DispatchError> {
    if context.is_none() {
        return Ok(None);
    }
    match requests::agents(context, state).await {
        Ok(()) => return Ok(None),
        Err(DispatchError::NotImplemented) => {
            Context::assert_unhandled(context, "provider::agents");
        }
        Err(error) => return Err(error),
    }
    match requests::runtime(context, state, connection).await {
        Ok(completion) => return Ok(completion),
        Err(DispatchError::NotImplemented) => {
            Context::assert_unhandled(context, "provider::runtime");
        }
        Err(error) => return Err(error),
    }
    match requests::execution(context, state).await {
        Ok(()) => return Ok(None),
        Err(DispatchError::NotImplemented) => {
            Context::assert_unhandled(context, "provider::execution");
        }
        Err(error) => return Err(error),
    }
    match requests::timeline(context, state, connection).await {
        Ok(()) => return Ok(None),
        Err(DispatchError::NotImplemented) => {
            Context::assert_unhandled(context, "provider::timeline");
        }
        Err(error) => return Err(error),
    }
    match requests::catalog(context, state) {
        Ok(()) => return Ok(None),
        Err(DispatchError::NotImplemented) => {
            Context::assert_unhandled(context, "provider::catalog");
        }
        Err(error) => return Err(error),
    }
    Err(DispatchError::NotImplemented)
}
