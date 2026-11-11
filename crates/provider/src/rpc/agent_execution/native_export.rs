//! Typed native HTML export requests executed on the owning Agent's session lane.

use super::ExecutionState;
use crate::ports::native_html_export::{ExportedNativeHtml, Request};
use crate::rpc::ErrorCode;
use crate::service::agent_execution::operation::Operation;

type ExportReply = tokio::sync::oneshot::Sender<Result<ExportedNativeHtml, ErrorCode>>;
type ExportReceiver = tokio::sync::oneshot::Receiver<Result<ExportedNativeHtml, model::ErrorCode>>;

impl ExecutionState {
    /// Start the provider's own HTML render for one live OMP session and deliver its artifact
    /// through the typed reply endpoint. The render never prompts, resumes or interrupts the
    /// conversation. The owned request is routed directly (no JSON round-trip); the caller
    /// identity is re-resolved to the canonical Ait Agent id exactly as a session request.
    ///
    /// This returns as soon as the render is admitted: the completion is produced by the
    /// lane's normal manager polling, so awaiting it here would deadlock the lane (the poll
    /// that fills the reply cannot run while the lane awaits this request). The receiver is
    /// therefore forwarded to the caller off-lane; abandoning the caller drops the reply and
    /// the adapter still removes its owned staging file on the late response or close.
    pub(crate) async fn native_html_export(&mut self, request: Request, reply: ExportReply) {
        let id = match self.resolve_for(Operation::NativeHtmlExport, &request.agent_id) {
            Ok(id) => id,
            Err(error) => {
                let _ = reply.send(Err(error));
                return;
            }
        };
        let receiver: ExportReceiver = match self.manager.begin_native_html_export(&id).await {
            Ok(receiver) => receiver,
            Err(error) => {
                let _ = reply.send(Err(super::map_manager(&error)));
                return;
            }
        };
        if reply.is_closed() {
            self.manager.cancel_native_html_export(&id);
            return;
        }
        // Forward off-lane so the lane returns to its poll loop, which drives the render to
        // completion. If the caller disappears the send fails silently; the adapter's late
        // response/close cleanup still owns the staging file.
        tokio::spawn(async move {
            let result = receiver
                .await
                .unwrap_or(Err(model::ErrorCode::AgentIo))
                .map_err(ErrorCode::from);
            let _ = reply.send(result);
        });
    }
}
