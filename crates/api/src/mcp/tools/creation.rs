//! Workspace placement and native launch translation without persistent MCP credentials.

use model::ErrorCode;
use serde_json::{Value, json};

use crate::Shared;
use crate::connection::rpc;

use super::{queries, rename, take};

/// Fill missing directory and placement in `arguments` from `caller` or an active Workspace.
/// # Errors
/// Returns missing context, unknown Workspace or directory RPC failures from `state`.
pub(super) async fn directory(
    state: &Shared,
    arguments: &mut Value,
    caller: Option<&Value>,
) -> Result<(), ErrorCode> {
    if arguments.get("workspaceId").is_none()
        && let Some(workspace) = caller.and_then(|caller| caller.get("workspaceId"))
    {
        arguments["workspaceId"] = workspace.clone();
        if arguments.get("cwd").is_none()
            && let Some(cwd) = caller.and_then(|caller| caller.get("cwd"))
        {
            arguments["cwd"] = cwd.clone();
        }
    }
    if arguments.get("cwd").is_some() {
        return Ok(());
    }
    if let Some(id) = arguments.get("workspaceId") {
        let workspace =
            rpc::execute(state, "workspace.get.request", json!({"workspaceId":id})).await?;
        if !workspace["workspace"]["workspaceDirectory"].is_string() {
            return Err(ErrorCode::InvalidMessage);
        }
        arguments["cwd"] = workspace["workspace"]["workspaceDirectory"].clone();
    } else if let Some(cwd) = caller.and_then(|caller| caller.get("cwd")) {
        arguments["cwd"] = cwd.clone();
    } else {
        return Err(ErrorCode::InvalidMessage);
    }
    Ok(())
}

/// Translate `arguments` into an Agent creation, inheriting `caller` context in `state`.
/// # Errors
/// Returns malformed provider/model, missing placement or native creation failures.
pub(super) async fn agent(
    state: &Shared,
    mut arguments: Value,
    caller: Option<&Value>,
) -> Result<Value, ErrorCode> {
    let (provider, model) = provider_model(&arguments["provider"], true)?;
    let provider = provider.to_owned();
    let model = model.map(str::to_owned);
    directory(state, &mut arguments, caller).await?;
    let mut config = take(&mut arguments, "settings")?.unwrap_or_else(|| json!({}));
    if config.get("model").is_some() {
        return Err(ErrorCode::InvalidMessage);
    }
    rename(&mut config, "features", "featureValues")?;
    config["provider"] = json!(provider);
    config["model"] = json!(model);
    config["cwd"] = take(&mut arguments, "cwd")?.ok_or(ErrorCode::InvalidMessage)?;
    config["title"] = take(&mut arguments, "title")?.ok_or(ErrorCode::InvalidMessage)?;
    take(&mut arguments, "provider")?;
    arguments["config"] = config;
    if let Some(caller) = caller {
        arguments["callerAgentId"] = caller["id"].clone();
    }
    rpc::execute(state, "agent.create.request", arguments).await
}

/// Split `value` at its first slash, requiring a model when `required_model` is true.
/// # Errors
/// Rejects non-text, blank segments or a missing required model.
pub(super) fn provider_model(
    value: &Value,
    required_model: bool,
) -> Result<(&str, Option<&str>), ErrorCode> {
    let value = value.as_str().ok_or(ErrorCode::InvalidMessage)?.trim();
    if let Some((provider, model)) = value.split_once('/') {
        let (provider, model) = (provider.trim(), model.trim());
        if provider.is_empty() || model.is_empty() {
            return Err(ErrorCode::InvalidMessage);
        }
        return Ok((provider, Some(model)));
    }
    if value.is_empty() || required_model {
        return Err(ErrorCode::InvalidMessage);
    }
    Ok((value, None))
}

/// Return a Workspace source from `arguments`, inheriting `caller`'s directory when omitted.
/// # Errors
/// Rejects missing directories and conflicting branch or change-request selections.
pub(super) fn workspace(mut arguments: Value, caller: Option<&Value>) -> Result<Value, ErrorCode> {
    let path = take(&mut arguments, "path")?
        .or_else(|| caller.and_then(|caller| caller.get("cwd")).cloned());
    let project = take(&mut arguments, "projectId")?;
    let isolation = take(&mut arguments, "isolation")?.ok_or(ErrorCode::InvalidMessage)?;
    let mut source = if isolation == "local" {
        if [
            "mode",
            "worktreeSlug",
            "branchName",
            "baseBranch",
            "branch",
            "prNumber",
            "forge",
        ]
        .iter()
        .any(|key| arguments.get(key).is_some())
        {
            return Err(ErrorCode::InvalidMessage);
        }
        json!({"kind":"directory","path":path.ok_or(ErrorCode::InvalidMessage)?})
    } else {
        worktree(&mut arguments, path)?
    };
    if let Some(project) = project {
        source["projectId"] = project;
    }
    arguments["source"] = source;
    Ok(arguments)
}

fn worktree(arguments: &mut Value, path: Option<Value>) -> Result<Value, ErrorCode> {
    let mode = take(arguments, "mode")?.unwrap_or_else(|| json!("branch-off"));
    let mut source = json!({"kind":"worktree"});
    if let Some(path) = path {
        source["cwd"] = path;
    }
    for field in ["worktreeSlug", "branchName", "baseBranch"] {
        if let Some(value) = take(arguments, field)? {
            source[field] = value;
        }
    }
    let branch = take(arguments, "branch")?;
    let number = take(arguments, "prNumber")?;
    let forge = take(arguments, "forge")?;
    match mode.as_str() {
        Some("branch-off") if branch.is_none() && number.is_none() && forge.is_none() => {
            source["action"] = json!("branch-off");
        }
        Some("checkout-branch") if number.is_none() && forge.is_none() => {
            source["action"] = json!("checkout");
            source["refName"] = branch.ok_or(ErrorCode::InvalidMessage)?;
        }
        Some("checkout-pr") if branch.is_none() => {
            source["action"] = json!("checkout");
            let number = number.ok_or(ErrorCode::InvalidMessage)?;
            if let Some(forge) = forge {
                source["checkoutSource"] =
                    json!({"kind":"change_request","forge":forge,"number":number});
            } else {
                source["githubPrNumber"] = number;
            }
        }
        _ => return Err(ErrorCode::InvalidMessage),
    }
    Ok(source)
}

/// Return the resolved public Agent snapshot for `id` from `state`.
/// # Errors
/// Returns missing Agent or existing lookup failures.
pub(super) async fn require_success(state: &Shared, id: &Value) -> Result<Value, ErrorCode> {
    let value = rpc::execute(state, "agent.get.request", json!({"agentId":id})).await?;
    if queries::failed(&value) || !value["agent"].is_object() {
        return Err(ErrorCode::InvalidMessage);
    }
    Ok(value["agent"].clone())
}
