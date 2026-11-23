//! Admit native exports on the session lane and forward completion without blocking the lane.

use super::ExecutionState;
use crate::ports::native_html_export::Request;
use crate::service::agent_execution::NativeExportReply;
use crate::service::agent_execution::operation::Operation;

impl ExecutionState {
    pub(crate) fn native_html_export(&mut self, request: &Request, mut reply: NativeExportReply) {
        if reply.is_closed() {
            return;
        }
        let result = self.resolve_for(Operation::NativeHtmlExport, &request.agent_id);
        let id = match result {
            Ok(id) => id,
            Err(error) => {
                let _ = reply.send(Err(error));
                return;
            }
        };
        let receiver = match self.manager.begin_native_html_export(&id) {
            Ok(receiver) => receiver,
            Err(error) => {
                let _ = reply.send(Err(error));
                return;
            }
        };
        if reply.is_closed() {
            self.manager.cancel_native_html_export(&id);
            return;
        }
        // Dropping the internal receiver makes the manager cancel at its next poll. This keeps
        // an abandoned caller from retaining the adapter's staging file or reply indefinitely.
        tokio::spawn(async move {
            tokio::select! {
                () = reply.closed() => {},
                result = receiver => {
                    let result = result.unwrap_or(Err(crate::rpc::ErrorCode::AgentIo));
                    let _ = reply.send(result);
                }
            }
        });
    }
}
