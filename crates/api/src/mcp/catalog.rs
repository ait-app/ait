//! Tool metadata and schemas, filtered by installed RPC capabilities.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use domain::bot::permissions::{Holder, ToolAccess, tool_access};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::Shared;

use super::Scope;

/// A retired name a tool still answers to in `tools/call`, with the argument names it used
/// (ADR-295). Aliases are not listed by `tools/list`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Alias {
    /// The retired tool name.
    pub(super) name: String,
    /// Retired argument names mapped to the current ones.
    #[serde(default)]
    arguments: BTreeMap<String, String>,
    /// Calls made under this name since the daemon started.
    #[serde(skip)]
    uses: AtomicU64,
}

impl Alias {
    /// Rename retired top-level `arguments` to their current names; a current name already
    /// present wins.
    pub(super) fn rewrite(&self, arguments: &mut Value) {
        let Some(arguments) = arguments.as_object_mut() else {
            return;
        };
        for (retired, current) in &self.arguments {
            if let Some(value) = arguments.remove(retired) {
                arguments.entry(current.as_str()).or_insert(value);
            }
        }
    }

    /// Count one call under this name and log the count each time it doubles, so the log shows
    /// whether a retired name is still in use without growing with every call.
    pub(super) fn record(&self, tool: &str) {
        let uses = self.uses.fetch_add(1, Ordering::Relaxed) + 1;
        if uses.is_power_of_two() {
            tracing::info!(alias = %self.name, tool, uses, "retired MCP tool name called");
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// One embedded tool definition with its complete required capability set.
pub(super) struct Tool {
    /// Stable MCP name used by discovery and execution: `<domain>[_<object>]_<verb>`.
    pub(super) name: String,
    /// Short human-readable name for clients.
    title: String,
    /// Retired names still accepted by `tools/call`.
    #[serde(default)]
    pub(super) aliases: Vec<Alias>,
    description: String,
    /// RPC/event methods which must all be installed before exposing this tool.
    pub(super) methods: Vec<String>,
    /// Closed input schema using the vocabulary supported by the private validator.
    pub(super) input_schema: Value,
    read_only: bool,
    /// Whether the tool archives, kills or deletes something.
    #[serde(default)]
    destructive: bool,
    /// Whether sibling Hosts of the account may call it, and host callers may route it to them.
    #[serde(default)]
    pub(super) peer: bool,
}

/// Arguments only Bot sessions may pass, by tool; every other caller's schema omits them, so
/// validation refuses them (ADR-274: only the Bot names the ticket a new Agent joins).
const BOT_ARGUMENTS: [(&str, &str); 1] = [("agent_create", "ticketId")];

/// Argument naming the Host, by its `serverId`, that runs a call of a tool marked for peers;
/// only host callers are offered it.
pub(super) const HOST_ARGUMENT: &str = "hostId";

impl Tool {
    /// Return public MCP metadata for `scope`'s caller without exposing internal RPC method
    /// mappings.
    pub(super) fn descriptor(&self, scope: Option<&Scope>) -> Value {
        json!({
            "name":self.name,"title":self.title,"description":self.description,
            "inputSchema":self.schema(scope),
            "annotations":{"title":self.title,"readOnlyHint":self.read_only,
                "destructiveHint":self.destructive,"openWorldHint":false}
        })
    }

    /// The input schema `scope`'s caller sees and is validated against: without arguments
    /// reserved for Bot sessions unless the caller is one, and, for host callers of a tool
    /// marked for peers, with the optional `hostId` that runs it on another Host.
    pub(super) fn schema(&self, scope: Option<&Scope>) -> Cow<'_, Value> {
        let bot = scope.is_some_and(|scope| scope.caller.kind.holder() == Holder::Bot);
        let withheld = !bot && BOT_ARGUMENTS.iter().any(|(tool, _)| *tool == self.name);
        let routable = scope.is_none() && self.peer;
        if !withheld && !routable {
            return Cow::Borrowed(&self.input_schema);
        }
        let mut schema = self.input_schema.clone();
        if let Some(properties) = schema["properties"].as_object_mut() {
            // A Bot session never gets here: it sees its schema as written.
            for (_, argument) in BOT_ARGUMENTS.iter().filter(|(tool, _)| *tool == self.name) {
                properties.remove(*argument);
            }
            if routable {
                properties.insert(
                    HOST_ARGUMENT.to_owned(),
                    json!({"type":"string","minLength":1}),
                );
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

/// Find the tool called `name`, by its current name or a retired one; the alias is returned so
/// the caller can rewrite retired arguments and record the use.
pub(super) fn resolve(name: &str) -> Option<(&'static Tool, Option<&'static Alias>)> {
    all().iter().find_map(|tool| {
        if tool.name == name {
            return Some((tool, None));
        }
        let alias = tool.aliases.iter().find(|alias| alias.name == name)?;
        Some((tool, Some(alias)))
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

/// Find installed tool `name` marked for calls from sibling Hosts, by its current name only.
pub(super) fn for_peers(state: &Shared, name: &str) -> Option<&'static Tool> {
    installed(state).find(|tool| tool.peer && tool.name == name)
}

#[cfg(test)]
mod tests;

/// Iterate tools whose complete method set is implemented in `state`.
fn installed(state: &Shared) -> impl Iterator<Item = &'static Tool> + '_ {
    all().iter().filter(|tool| {
        tool.methods
            .iter()
            .all(|method| state.info.implemented_capabilities.contains(method))
    })
}
