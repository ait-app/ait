//! Read-only native discovery has its own bounded lane, separate from live Agent execution.

use std::collections::BTreeMap;
use std::sync::Arc;

use metadata::service::session::SessionEvents;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::ports::agent_session::AgentClient;
use crate::rpc::ErrorCode;
use crate::service::provider_catalog::Catalog;

#[derive(Debug)]
/// One admitted discovery request, retained even when its response receiver is dropped.
pub(super) struct Request {
    /// Canonical provider catalog method.
    pub(super) method: String,
    /// Method-specific validated-by-catalog arguments.
    pub(super) params: Value,
    /// Completion sent only after discovery/cache publication finishes.
    pub(super) reply: oneshot::Sender<Result<Value, ErrorCode>>,
}

/// Select catalog methods without routing Agent or native-session lifecycle commands here.
pub(super) fn handles(method: &str) -> bool {
    matches!(
        method,
        "provider.available.list.request"
            | "provider.models.list.request"
            | "provider.modes.list.request"
            | "provider.features.list.request"
            | "provider.snapshot.get.request"
            | "provider.snapshot.refresh.request"
    )
}

/// Serialize cache access while discovery yields independently of Agent execution.
/// Shutdown closes admission and drains already accepted work before this future returns.
pub(super) async fn serve(
    mut catalog: Catalog,
    clients: BTreeMap<String, Arc<dyn AgentClient>>,
    events: SessionEvents,
    mut requests: mpsc::Receiver<Request>,
    shutdown: CancellationToken,
) {
    loop {
        let request = tokio::select! {
            biased;
            () = shutdown.cancelled(), if !requests.is_closed() => {
                requests.close();
                continue;
            }
            request = requests.recv() => request,
        };
        let Some(request) = request else { break };
        // Complete accepted discovery even if its caller disconnects: later readers share
        // the cache, and explicit refresh retains its existing publication semantics.
        let result = catalog
            .execute(&clients, &events, &request.method, request.params)
            .await
            .map_err(Into::into);
        let _ = request.reply.send(result);
    }
}
