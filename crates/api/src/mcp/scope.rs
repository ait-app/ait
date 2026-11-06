//! Authorization and argument rewriting for Bot-scoped MCP callers.

use bot::mcp::{Caller, CallerKind, Grant};
use domain::bot::permissions::BotPermissions;
use model::ErrorCode;
use serde_json::{Value, json};

use crate::Shared;

use super::tools::creation;

/// A scoped credential's identity with the grant read once for the current request.
#[derive(Debug)]
pub(super) struct Scope {
    /// Bot session or subtree Agent that presented the credential.
    pub(super) caller: Caller,
    /// Current tool groups, mode cap and Workspaces of the caller's root Workspace.
    pub(super) grant: Grant,
}

impl Scope {
    /// Whether `tool` is granted; Bot-only tools are granted to Bot sessions, never subtrees.
    pub(super) fn allows(&self, tool: &str) -> bool {
        let bot = matches!(self.caller.kind, CallerKind::Bot { .. });
        self.grant.permissions.allows(tool, bot)
    }
}

/// Rewrite scoped `arguments` for `tool` before execution.
///
/// Context a scoped caller omits comes from its root Workspace: the directory for
/// `inspect_provider` and `list_models`, the Workspace for `create_terminal`, `list_terminals`
/// and `create_schedule`. `create_agent` must use a capped provider and mode, and an omitted
/// mode becomes the provider's default.
/// # Errors
/// Returns `Forbidden` for a provider or mode outside the cap and propagates root Workspace
/// lookup failures.
pub(super) async fn confine(
    state: &Shared,
    scope: &Scope,
    tool: &str,
    arguments: &mut Value,
) -> Result<(), ErrorCode> {
    let root = &scope.caller.root;
    match tool {
        "inspect_provider" | "list_models" if arguments.get("cwd").is_none() => {
            arguments["cwd"] = creation::workspace_directory(state, &json!(root)).await?;
        }
        "create_terminal" | "list_terminals" | "create_schedule"
            if arguments.get("workspaceId").is_none() =>
        {
            arguments["workspaceId"] = json!(root);
        }
        "create_agent" => cap_mode(&scope.grant.permissions, arguments)?,
        _ => {}
    }
    Ok(())
}

/// Require `create_agent`'s provider and `settings.modeId` to be within `permissions`' cap,
/// selecting the provider's default mode when none is requested.
/// # Errors
/// Returns `InvalidMessage` for a malformed provider and `Forbidden` outside the cap.
fn cap_mode(permissions: &BotPermissions, arguments: &mut Value) -> Result<(), ErrorCode> {
    let (provider, _) = creation::provider_model(&arguments["provider"], true)?;
    let requested = arguments
        .pointer("/settings/modeId")
        .and_then(Value::as_str);
    let mode = permissions
        .mode(provider, requested)
        .map_err(|_| ErrorCode::Forbidden)?;
    arguments["settings"]["modeId"] = json!(mode);
    Ok(())
}

#[cfg(test)]
mod tests;
