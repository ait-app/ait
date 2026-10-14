//! Trusted Bot execution facade; these operations are never advertised as client Agent RPCs.

use domain::bot::{ExecutionConfig, SessionOwner};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::agent_execution::AgentExecution;

/// An owner-checked operation on a hidden provider session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    /// Create once, or reuse an existing session with the exact same owner.
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
