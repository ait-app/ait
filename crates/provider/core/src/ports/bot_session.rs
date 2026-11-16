//! Bot-session capabilities an adapter publishes for trusted Bot execution.

use serde::{Deserialize, Serialize};

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
