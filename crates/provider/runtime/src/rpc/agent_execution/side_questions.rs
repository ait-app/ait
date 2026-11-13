//! Typed BTW side-question RPC handlers (shared BTW contract).
//!
//! Typed methods cover ask, cancel, history and transfer without a string-action gateway.
//! Identity resolution and observed-agent guards match the other agent
//! operations; the manager owns receipts and one-active enforcement.

use serde_json::Value;

use super::{ErrorCode, ExecutionState, decode, only};
use provider_core::protocol::side_questions::{
    SideAskRequest, SideCancelRequest, SideHistoryRequest, SideTransferRequest,
};

impl ExecutionState {
    pub(super) async fn side_operation(
        &mut self,
        operation: crate::service::agent_execution::operation::Operation,
        params: Value,
    ) -> Result<Value, ErrorCode> {
        use crate::service::agent_execution::operation::Operation;
        match operation {
            Operation::SideAsk => self.side_ask(params).await,
            Operation::SideCancel => self.side_cancel(params).await,
            Operation::SideHistory => self.side_history(params).await,
            Operation::SideTransfer => self.side_transfer(params),
            _ => Err(ErrorCode::MethodNotFound),
        }
    }

    pub(super) async fn side_ask(&mut self, params: Value) -> Result<Value, ErrorCode> {
        only(&params, &["agentId", "requestId", "question", "recordId"])?;
        let mut request: SideAskRequest = decode(params)?;
        request.agent_id = self.resolve(&request.agent_id)?;
        let response = self.manager.side_ask(request).await?;
        serde_json::to_value(response).map_err(|_| ErrorCode::AgentIo)
    }

    pub(super) async fn side_cancel(&mut self, params: Value) -> Result<Value, ErrorCode> {
        only(&params, &["agentId", "recordId"])?;
        let mut request: SideCancelRequest = decode(params)?;
        request.agent_id = self.resolve(&request.agent_id)?;
        let response = self.manager.side_cancel(request).await?;
        serde_json::to_value(response).map_err(|_| ErrorCode::AgentIo)
    }

    pub(super) fn side_transfer(&mut self, params: Value) -> Result<Value, ErrorCode> {
        only(&params, &["agentId", "recordId"])?;
        let mut request: SideTransferRequest = decode(params)?;
        request.agent_id = self.resolve(&request.agent_id)?;
        let response = self.manager.side_transfer(&request)?;
        serde_json::to_value(response).map_err(|_| ErrorCode::AgentIo)
    }

    pub(super) async fn side_history(&mut self, params: Value) -> Result<Value, ErrorCode> {
        only(&params, &["agentId"])?;
        let mut request: SideHistoryRequest = decode(params)?;
        request.agent_id = self.resolve(&request.agent_id)?;
        let response = self.manager.side_history(request).await?;
        serde_json::to_value(response).map_err(|_| ErrorCode::AgentIo)
    }
}
