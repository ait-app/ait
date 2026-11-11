//! Typed native HTML export requests executed on the owning Agent's session lane.

use serde_json::Value;

use super::ExecutionState;
use crate::ports::native_html_export::ExportedNativeHtml;
use crate::rpc::ErrorCode;
use crate::service::agent_execution::operation::Operation;
use crate::support::rpc::decode;

type ExportReply = tokio::sync::oneshot::Sender<Result<ExportedNativeHtml, ErrorCode>>;
type ExportReceiver =
    tokio::sync::oneshot::Receiver<Result<ExportedNativeHtml, model::ErrorCode>>;

impl ExecutionState {
    /// Start the provider's own HTML render for one live OMP session and deliver its artifact
    /// through the typed reply endpoint. The render never prompts, resumes or interrupts the
    /// conversation; abandoning the request withdraws the export and the adapter still
    /// removes its owned staging file.
    pub(crate) async fn native_html_export(&mut self, params: Value, reply: ExportReply) {
        let request: crate::ports::native_html_export::Request = match decode(params) {
            Ok(request) => request,
            Err(error) => {
                let _ = reply.send(Err(error));
                return;
            }
        };
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
        let outcome = receiver.await;
        if outcome.is_err() || reply.is_closed() {
            // The waiting request disappeared; withdraw the render and keep its cleanup.
            self.manager.cancel_native_html_export(&id);
            return;
        }
        let result = outcome
            .unwrap_or(Err(model::ErrorCode::AgentIo))
            .map_err(ErrorCode::from);
        let _ = reply.send(result);
    }
}