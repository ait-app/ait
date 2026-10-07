//! Composition of Workspace provisioning and the initial native Agent under one receipt.

use std::sync::{Arc, Mutex};

use metadata::rpc::directory::WorkspaceCreated;
use model::creation::protocol::Kind;
use model::events::Subscription;
use model::{Context, DispatchError, ErrorCode};
use serde_json::{Value, json};

use crate::Shared;

pub(super) async fn request(
    context: &mut Option<Context<'_>>,
    state: &Shared,
) -> Result<(), DispatchError> {
    if context.is_none() {
        return Ok(());
    }
    let Some(mut context) = context.take_if(|context| {
        context.request.method == "workspace.create.request"
            && context
                .request
                .params
                .get("agent")
                .is_some_and(|agent| !agent.is_null())
    }) else {
        return Err(DispatchError::NotImplemented);
    };
    let input = match prepare(&mut context, state).await {
        Ok(input) => input,
        Err(error) => return context.respond(Err(error)).map_err(Into::into),
    };
    if let Some(subscription) = &input.subscription {
        subscription.activate()?;
    }
    let result = create(&mut context, state, &input).await;
    match result {
        Ok(value) => {
            let event = (!value["workspace"].is_null())
                .then(|| json!({"kind":"upsert","workspace":value["workspace"]}));
            context.workspace(value, event)?;
            Ok(())
        }
        Err(error) => context.respond(Err(error)).map_err(Into::into),
    }
}

struct Input {
    agent: Value,
    intent: Value,
    source: Value,
    subscription: Option<Subscription>,
}

async fn prepare(context: &mut Context<'_>, state: &Shared) -> Result<Input, ErrorCode> {
    if state.provider.agent_execution.is_none() {
        return Err(ErrorCode::UnsupportedCapability);
    }
    let request: model::workspace::protocol::directory::WorkspaceCreateRequest =
        serde_json::from_value(context.request.params.clone())
            .map_err(|_| ErrorCode::InvalidMessage)?;
    let agent = request.agent.ok_or(ErrorCode::InvalidMessage)?;
    let intent = provider::rpc::agent_execution::workspace_creation::validate(&agent)?;
    let observe = request.subscribe.unwrap_or(false);
    if observe && context.available_subscriptions == 0 {
        return Err(ErrorCode::ResourceExhausted);
    }
    let key = request
        .idempotency_key
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    validate_agent_identity(state, &agent, &key).await?;
    context.request.params["idempotencyKey"] = json!(key);
    let subscription = if observe {
        let creations = state.metadata.creations.clone();
        let outbound = context.outbound.clone();
        Some(
            context
                .runtime
                .run(
                    Some(Arc::new(Mutex::new(creations))),
                    ErrorCode::RegistryIo,
                    move |creations| creations.observe(Kind::Workspace, &key, outbound),
                )
                .await?,
        )
    } else {
        None
    };
    Ok(Input {
        agent,
        intent,
        source: context.request.params["source"].clone(),
        subscription,
    })
}

async fn validate_agent_identity(
    state: &Shared,
    agent: &Value,
    key: &str,
) -> Result<(), ErrorCode> {
    let Some(id) = agent["agentId"].as_str().map(str::to_owned) else {
        return Ok(());
    };
    let creations = state.metadata.creations.clone();
    let key = key.to_owned();
    let existing = state
        .runtime
        .run(
            Some(Arc::new(Mutex::new(creations))),
            ErrorCode::RegistryIo,
            move |creations| creations.snapshot(Kind::Workspace, &key),
        )
        .await?;
    if existing.is_none() && provider::dispatch::contains_identity(&state.provider, id).await? {
        return Err(ErrorCode::IdempotencyConflict);
    }
    Ok(())
}

async fn create(
    context: &mut Context<'_>,
    state: &Shared,
    input: &Input,
) -> Result<Value, ErrorCode> {
    let intent = input.intent.clone();
    let params = std::mem::take(&mut context.request.params);
    let mut created = context
        .runtime
        .run(
            state.metadata.directory.clone(),
            ErrorCode::RegistryIo,
            move |directory| {
                metadata::rpc::directory::workspace_creation_with_agent(directory, params, intent)
                    .map_err(Into::into)
            },
        )
        .await?;
    if let Some(workspace) = created.created_worktree_id.clone()
        && state.metadata.workspace_automation.is_some()
    {
        // Setup is a separate workspace resource; its failure does not erase a provisioned checkout.
        let _ = state
            .runtime
            .run_queued(
                state.metadata.workspace_automation.clone(),
                ErrorCode::RegistryIo,
                move |automation| {
                    automation
                        .start_created_setup(&workspace)
                        .map(|_| ())
                        .map_err(|_| ErrorCode::RegistryIo)
                },
            )
            .await;
    }
    if let Some(receipt) = created.pending_agent.take() {
        finish(state, input, &mut created, receipt).await?;
    } else {
        await_replay(state, &mut created).await?;
    }
    Ok(created.value)
}

async fn await_replay(state: &Shared, created: &mut WorkspaceCreated) -> Result<(), ErrorCode> {
    let mut snapshot: model::creation::protocol::Snapshot =
        serde_json::from_value(created.value["creation"].clone())
            .map_err(|_| ErrorCode::RegistryIo)?;
    let service = Arc::new(Mutex::new(state.metadata.creations.clone()));
    while !matches!(snapshot.phase.as_str(), "completed" | "failed") {
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        let key = snapshot.idempotency_key.clone();
        snapshot = state
            .runtime
            .run_queued(
                Some(service.clone()),
                ErrorCode::RegistryIo,
                move |creations| {
                    creations
                        .snapshot(Kind::Workspace, &key)?
                        .ok_or(ErrorCode::RegistryIo)
                },
            )
            .await?;
    }
    super::creation_receipts::validate_agent(state, &snapshot).await?;
    created.value["agent"] = json!(snapshot.agent);
    created.value["error"] = json!(snapshot.error);
    created.value["creation"] = json!(snapshot);
    Ok(())
}

async fn finish(
    state: &Shared,
    input: &Input,
    created: &mut WorkspaceCreated,
    receipt: model::creation::protocol::Snapshot,
) -> Result<(), ErrorCode> {
    let execution = state
        .provider
        .agent_execution
        .as_ref()
        .ok_or(ErrorCode::UnsupportedCapability)?;
    let result = execution
        .execute(
            "internal.workspace.agent.create",
            json!({
                "agent":input.agent,"creation":receipt,"source":input.source,
            }),
        )
        .await;
    let creations = state.metadata.creations.clone();
    let snapshot = state
        .runtime
        .run_queued(
            Some(Arc::new(Mutex::new(creations))),
            ErrorCode::RegistryIo,
            move |creations| {
                let mut snapshot = creations
                    .snapshot(Kind::Workspace, &receipt.idempotency_key)?
                    .ok_or(ErrorCode::RegistryIo)?;
                if let Err(error) = result
                    && snapshot.phase != "failed"
                    && snapshot.phase != "completed"
                {
                    snapshot =
                        creations.advance(&snapshot, "failed", None, Some(error.to_string()))?;
                }
                Ok(snapshot)
            },
        )
        .await?;
    created.value["agent"] = json!(snapshot.agent);
    created.value["error"] = json!(snapshot.error);
    created.value["creation"] = json!(snapshot);
    Ok(())
}
