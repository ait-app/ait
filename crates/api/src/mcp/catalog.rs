//! Tool metadata and schemas, filtered by installed RPC capabilities.

use std::sync::OnceLock;

use domain::bot::permissions::{ToolAccess, tool_access};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::Shared;

use super::Scope;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// One embedded tool definition with its complete required capability set.
pub(super) struct Tool {
    /// Stable MCP name used by discovery and execution.
    pub(super) name: String,
    description: String,
    /// RPC/event methods which must all be installed before exposing this tool.
    pub(super) methods: Vec<String>,
    /// Closed input schema using the vocabulary supported by the private validator.
    pub(super) input_schema: Value,
    read_only: bool,
}

impl Tool {
    /// Return public MCP metadata without exposing internal RPC method mappings.
    pub(super) fn descriptor(&self) -> Value {
        json!({
            "name":self.name,"description":self.description,"inputSchema":self.input_schema,
            "annotations":{"readOnlyHint":self.read_only,"openWorldHint":false}
        })
    }
}

/// Return the once-decoded immutable catalog embedded in this binary.
pub(super) fn all() -> &'static [Tool] {
    static TOOLS: OnceLock<Vec<Tool>> = OnceLock::new();
    TOOLS.get_or_init(|| {
        serde_json::from_str(include_str!("catalog.json"))
            .expect("embedded MCP tool catalog is valid JSON")
    })
}

/// Iterate installed tools visible to a caller: the host sees every tool except Bot-only ones,
/// a scoped caller only the tools its current grant allows.
pub(super) fn granted<'a>(
    state: &'a Shared,
    scope: Option<&'a Scope>,
) -> impl Iterator<Item = &'static Tool> + 'a {
    installed(state).filter(move |tool| {
        scope.map_or_else(
            || tool_access(&tool.name) != ToolAccess::BotOnly,
            |scope| scope.allows(&tool.name),
        )
    })
}

/// Iterate tools whose complete method set is implemented in `state`.
fn installed(state: &Shared) -> impl Iterator<Item = &'static Tool> + '_ {
    all().iter().filter(|tool| {
        tool.methods
            .iter()
            .all(|method| state.info.implemented_capabilities.contains(method))
    })
}
