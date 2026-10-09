//! Private native protocol facts; none of these types crosses the provider port.
use std::{path::PathBuf, sync::Arc};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Fault {
    AgentCapabilityUnsupported,
    ProviderFailed,
    RunRecoveryFailed,
    SessionBusy,
    RunCancelled,
    RunLimitExceeded,
    ToolUseRequiresAssistant,
    ToolCallDuplicate,
}

#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("{message}")]
pub(super) struct ProtocolError {
    pub code: Fault,
    pub message: &'static str,
}

#[derive(Clone, Debug)]
pub(super) struct Model {
    pub id: String,
    pub name: String,
    pub reasoning_efforts: Vec<String>,
}

#[derive(Clone)]
pub(super) struct Invocation {
    pub driver: String,
    pub request_id: String,
    pub session_id: Option<String>,
    pub input_id: String,
    pub prompt: String,
    pub instructions: Option<String>,
    pub cwd: PathBuf,
    pub model: String,
    pub reasoning_effort: Option<String>,
    pub full_access: bool,
    pub verify_settings: bool,
    pub agent: String,
    pub approvals: Arc<dyn ApprovalSink>,
    pub cancellation: CancellationToken,
    pub cancel_acknowledged: Arc<std::sync::atomic::AtomicBool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Role {
    User,
    Assistant,
    System,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum Content {
    Text { text: String },
    ToolCall(ToolCall),
    StructuredData { media_type: String, value: String },
    NativeContent(NativeContent),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct ToolCall {
    pub call_id: String,
    pub tool_name: String,
    pub arguments: String,
    pub provider_metadata: Option<Value>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ToolStatus {
    Succeeded,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct ToolOutput {
    pub call_id: String,
    pub status: ToolStatus,
    pub output: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct NativeContent {
    pub provider_kind: String,
    pub external_item_id: String,
    pub item_type: String,
    pub ordinal: u32,
    pub payload: Value,
    pub payload_schema_version: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct Record {
    pub id: String,
    pub role: Role,
    pub sub_messages: Vec<Content>,
    pub tool_result: Option<ToolOutput>,
    pub input_id: Option<String>,
    pub created_at: i64,
    pub metadata: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Outcome {
    Completed,
    Failed,
    Interrupted,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct Snapshot {
    pub driver: String,
    pub id: String,
    pub input_id: String,
    pub cwd: PathBuf,
    pub model: String,
    pub reasoning_effort: Option<String>,
    pub messages: Vec<Record>,
    pub outcome: Option<Outcome>,
}

#[derive(Clone, Debug)]
pub(super) enum ApprovalTarget {
    Command {
        command: String,
        cwd: String,
    },
    Files {
        paths: Vec<String>,
    },
    Native {
        action: String,
        resources: Vec<String>,
    },
}

#[derive(Clone, Debug)]
pub(super) struct ApprovalRequest {
    pub id: String,
    pub target: ApprovalTarget,
    pub save_resources: Vec<String>,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Decision {
    Approved,
    ApprovedAlways,
    Denied,
    Cancelled,
}

#[async_trait]
pub(super) trait ApprovalSink: Send + Sync {
    async fn decide(&self, request: ApprovalRequest) -> Result<Decision, ProtocolError>;
    async fn expire(&self, request: &ApprovalRequest) -> Result<(), ProtocolError>;
    async fn resolved(&self, _id: &str) -> Result<(), ProtocolError> {
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct DenyApprovals;
#[async_trait]
impl ApprovalSink for DenyApprovals {
    async fn decide(&self, _request: ApprovalRequest) -> Result<Decision, ProtocolError> {
        Ok(Decision::Denied)
    }
    async fn expire(&self, _request: &ApprovalRequest) -> Result<(), ProtocolError> {
        Ok(())
    }
}

#[derive(Debug)]
pub(super) enum ProgressEvent {
    Timeline(Box<crate::protocol::timeline::NativeItem>),
    /// `id` is the canonical native text item identity, not a protocol-specific message ID.
    TextDelta {
        id: String,
        delta: String,
    },
}

#[async_trait]
pub(super) trait ProgressSink: Send + Sync {
    async fn report(&self, event: ProgressEvent);
}
