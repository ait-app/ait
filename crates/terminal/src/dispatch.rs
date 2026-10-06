//! Concrete service state and crate-owned request dispatch.

use std::sync::{Arc, Mutex};

use model::outbound::QueueError;
use model::{Context, ErrorCode, Runtime};

use crate::capabilities::{Group, IMPLEMENTED_GROUPS};

/// Services installed for this capability crate, sharing server-wide runtime resources.
#[derive(Debug)]
pub struct State {
    /// Shared Tokio admission, cancellation and task tracking.
    pub runtime: Arc<Runtime>,
    /// Installed terminals service.
    pub terminals: Option<Arc<Mutex<crate::service::Terminals>>>,
}

impl std::ops::Deref for State {
    type Target = Runtime;

    fn deref(&self) -> &Runtime {
        &self.runtime
    }
}

/// Dispatch an admitted terminal request using its connection-owned streams.
/// Leaves `context` unchanged for other crates; takes it when this crate handles the method.
///
/// # Arguments
/// * `context` - Pending request, consumed only when this crate recognizes its method.
/// * `state` - Installed services and resources used to execute the request.
/// * `connection` - Connection-owned subscriptions and streams for this capability.
///
/// # Errors
/// Returns a delivery failure; terminal errors are sent as protocol responses.
pub async fn dispatch(
    context: &mut Option<Context<'_>>,
    state: &State,
    connection: &mut crate::connection::TerminalConnection,
) -> Result<(), QueueError> {
    let Some((group, context)) = Context::take_matching(context, IMPLEMENTED_GROUPS) else {
        return Ok(());
    };
    match group {
        Group::Terminal => {
            connection
                .request(
                    crate::connection::Request {
                        id: context.request.id,
                        method: context.request.method,
                        params: context.request.params,
                        available: context.available_subscriptions,
                    },
                    state,
                    context.outbound,
                )
                .await
        }
    }
}

/// Finish an admitted cross-capability close after the provider has closed its Agents.
/// # Errors
/// Returns terminal task admission or I/O errors, preserving per-terminal success results.
pub async fn close_many(state: &State, ids: Vec<String>) -> Result<serde_json::Value, ErrorCode> {
    crate::connection::run(state, move |terminals| {
        Ok(serde_json::json!(
            ids.into_iter()
                .map(|id| {
                    let success = terminals.kill(&id).is_ok();
                    serde_json::json!({"terminalId":id,"success":success})
                })
                .collect::<Vec<_>>()
        ))
    })
    .await
}

/// Apply a local HTTP activity report under the same bounded terminal admission as RPC calls.
/// # Errors
/// Returns shutdown, admission, or native terminal errors.
pub async fn report_activity(
    state: &State,
    terminal_id: String,
    token: secrecy::SecretString,
    activity: crate::activity::ReportState,
) -> Result<bool, ErrorCode> {
    use secrecy::ExposeSecret;
    crate::connection::run(state, move |terminals| {
        terminals.report_activity(&terminal_id, token.expose_secret(), activity)
    })
    .await
}

/// Clear focused-terminal attention after the metadata owner validates a visible heartbeat.
/// # Errors
/// Returns shutdown or admission errors; absent terminal services are ignored.
pub async fn clear_attention(state: &State, terminal_id: String) -> Result<(), ErrorCode> {
    if state.terminals.is_none() {
        return Ok(());
    }
    crate::connection::run(state, move |terminals| {
        terminals.clear_attention(&terminal_id);
        Ok(())
    })
    .await
}

/// Close processes whose Workspace or Project was archived before checkout removal.
/// # Errors
/// Returns admission, registry, or native cleanup failures while retaining failed ownership.
pub async fn reconcile_workspaces(
    state: &State,
    workspace_ids: Vec<String>,
) -> Result<(), ErrorCode> {
    if state.terminals.is_none() {
        return Ok(());
    }
    crate::connection::run(state, move |terminals| {
        terminals.close_workspaces(&workspace_ids)
    })
    .await
}
