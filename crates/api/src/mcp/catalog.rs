//! Tool metadata and schemas, filtered by installed RPC capabilities.

use std::borrow::Cow;
use std::sync::OnceLock;

use domain::bot::permissions::{Holder, ToolAccess, tool_access};
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

/// Arguments only Bot sessions may pass, by tool; every other caller's schema omits them, so
/// validation refuses them (ADR-274: only the Bot names the ticket a new Agent joins).
const BOT_ARGUMENTS: [(&str, &str); 1] = [("create_agent", "ticketId")];

impl Tool {
    /// Return public MCP metadata for `scope`'s caller without exposing internal RPC method
    /// mappings.
    pub(super) fn descriptor(&self, scope: Option<&Scope>) -> Value {
        json!({
            "name":self.name,"description":self.description,"inputSchema":self.schema(scope),
            "annotations":{"readOnlyHint":self.read_only,"openWorldHint":false}
        })
    }

    /// The input schema `scope`'s caller sees and is validated against: without arguments
    /// reserved for Bot sessions unless the caller is one.
    pub(super) fn schema(&self, scope: Option<&Scope>) -> Cow<'_, Value> {
        let bot = scope.is_some_and(|scope| scope.caller.kind.holder() == Holder::Bot);
        if bot || !BOT_ARGUMENTS.iter().any(|(tool, _)| *tool == self.name) {
            return Cow::Borrowed(&self.input_schema);
        }
        let mut schema = self.input_schema.clone();
        if let Some(properties) = schema["properties"].as_object_mut() {
            for (_, argument) in BOT_ARGUMENTS.iter().filter(|(tool, _)| *tool == self.name) {
                properties.remove(*argument);
            }
        }
        Cow::Owned(schema)
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

/// Iterate installed tools visible to a caller: the host sees every tool except Bot-only and
/// worker ones, a scoped caller only the tools its current grant allows.
pub(super) fn granted<'a>(
    state: &'a Shared,
    scope: Option<&'a Scope>,
) -> impl Iterator<Item = &'static Tool> + 'a {
    installed(state).filter(move |tool| {
        scope.map_or_else(
            || {
                !matches!(
                    tool_access(&tool.name),
                    ToolAccess::BotOnly | ToolAccess::Worker
                )
            },
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
