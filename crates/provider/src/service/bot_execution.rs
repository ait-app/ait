//! Trusted Bot execution facade; these operations are never advertised as client Agent RPCs.

use domain::bot::{ExecutionConfig, SessionOwner};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::agent_execution::AgentExecution;

/// An owner-checked operation on a hidden provider session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    /// Create once, or reuse an existing session with the exact same owner; returns [`Created`].
    ///
    /// The session's mode, tool preapprovals and system prompt follow the provider's
    /// [`BotSessionSupport`]; `config.mode_id` is ignored.
    Create {
        /// Stable preallocated runtime identity.
        agent_id: String,
        /// Native configuration.
        config: ExecutionConfig,
        /// Bot instructions and workspace memory context.
        instructions: String,
    },
    /// Admit a durable input using Provider's existing idempotency receipts.
    Send {
        /// Owner's hidden runtime.
        agent_id: String,
        /// Stable Bot receipt identity.
        message_id: String,
        /// Bounded input text.
        text: String,
    },
    /// Read committed execution and permission state.
    Status {
        /// Owner's hidden runtime.
        agent_id: String,
    },
    /// Cancel the current turn.
    Cancel {
        /// Owner's hidden runtime.
        agent_id: String,
    },
    /// Archive and close the owned session when resetting or archiving its Bot.
    Close {
        /// Owner's hidden runtime.
        agent_id: String,
    },
    /// Release an idle native writer while retaining its resumable private identity.
    Release {
        /// Owner's hidden runtime.
        agent_id: String,
    },
    /// Deny a pending native permission request. Bot turns cannot grant permissions.
    Permission {
        /// Owner's hidden runtime.
        agent_id: String,
        /// Native permission identity.
        permission_id: String,
        /// Must be exactly `{"behavior":"deny"}`; additional permissions are forbidden.
        response: Value,
    },
}

/// How the Bot's built-in instructions reach the native session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstructionDelivery {
    /// Written to, and kept current in, the session's updatable system prompt.
    SystemPrompt,
    /// Prefixed to the next input whenever the session has not seen the current instructions.
    TurnPrefix,
}

/// Bot-session capabilities of one provider adapter, published in the provider snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BotSessionSupport {
    /// How the Bot's instructions are delivered.
    pub instructions: InstructionDelivery,
    /// Strictest native read-only mode, if the provider has one.
    pub read_only_mode: Option<&'static str>,
    /// Whether exact `ait` tools can be preapproved through `tool_policy`.
    pub tool_preapproval: bool,
    /// Whether the session receives the injected `ait` HTTP MCP server at all.
    pub ait_tools: bool,
}

/// Result of [`Action::Create`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Created {
    /// Whether the native session and its record were created by this request.
    pub created: bool,
    /// How the caller must deliver the Bot's instructions to this session.
    pub instruction_delivery: InstructionDelivery,
    /// The existing session cannot reach the Bot's safe profile and was left unchanged;
    /// the caller must rotate to a new session.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reset_required: bool,
}

impl AgentExecution {
    /// Execute a trusted Bot operation, verifying its durable owner and workspace.
    ///
    /// `owner` identifies the Bot conversation; `action` contains no credentials.
    /// # Errors
    /// Returns validation, owner mismatch, inactive workspace, storage or native execution errors.
    pub async fn bot(
        &self,
        owner: &SessionOwner,
        action: Action,
    ) -> Result<Value, model::ErrorCode> {
        let action = serde_json::to_value(action).map_err(|_| model::ErrorCode::InvalidMessage)?;
        self.execute("internal.bot", json!({"owner":owner,"operation":action}))
            .await
            .map_err(Into::into)
    }
}
