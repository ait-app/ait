use std::sync::Arc;
use std::time::Duration;

use domain::bot::{Conversation, ExecutionConfig, SessionOwner, Turn, TurnStatus};
use provider::service::bot_execution::{Action, Created, InstructionDelivery};
use tokio_util::sync::CancellationToken;

use crate::ports::Executor;
use crate::service::Bots;

pub(crate) struct Run {
    pub(crate) turn: Turn,
    pub(crate) conversation: Conversation,
    pub(crate) owner: SessionOwner,
    pub(crate) config: ExecutionConfig,
    pub(crate) instructions: String,
    pub(crate) cancel: CancellationToken,
}

/// Failure of a turn whose session could not reach the Bot's safe profile even after rotation.
const UNSAFE_SESSION: &str =
    "Bot session could not be switched to its safe mode; reset this conversation";

pub(crate) async fn run(bots: Bots, executor: Arc<dyn Executor>, mut run: Run) {
    let conversation = run.conversation.id;
    let result = perform(&bots, &executor, &mut run).await;
    if result.is_err() {
        let _ = bots.progress(run.turn.id, TurnStatus::Uncertain, None,
            Some("Native execution outcome could not be confirmed; inspect and reset this conversation".into())).await;
    } else {
        let _ = executor
            .execute(
                run.owner.clone(),
                Action::Release {
                    agent_id: run.conversation.session_id.clone(),
                },
            )
            .await;
    }
    let _ = bots.finished(conversation).await;
}

async fn perform(
    bots: &Bots,
    executor: &Arc<dyn Executor>,
    run: &mut Run,
) -> Result<(), model::ErrorCode> {
    let Some(created) = prepare(bots, executor, run).await? else {
        return Ok(());
    };
    // Without an updatable system prompt the instructions ride on every input: a resumed native
    // session may not retain an earlier prefix, so no delivery is assumed.
    let text = if created.instruction_delivery == InstructionDelivery::TurnPrefix {
        crate::instructions::prefix(&run.instructions, &run.turn.text)
    } else {
        run.turn.text.clone()
    };
    let send = executor.execute(
        run.owner.clone(),
        Action::Send {
            agent_id: run.conversation.session_id.clone(),
            message_id: run.turn.id.to_string(),
            text,
        },
    );
    tokio::select! {
        () = run.cancel.cancelled() => return cancel(bots, executor, run).await,
        result = send => { result?; }
    }
    bots.progress(run.turn.id, TurnStatus::Running, None, None)
        .await?;
    loop {
        let status = tokio::select! {
            () = run.cancel.cancelled() => return cancel(bots, executor, run).await,
            result = executor.execute(run.owner.clone(), Action::Status { agent_id: run.conversation.session_id.clone() }) => result?,
        };
        if let Some(permissions) = status["final"]["pendingPermissions"].as_array()
            && !permissions.is_empty()
        {
            if deny_permissions(executor, run, permissions).await.is_err() {
                executor
                    .execute(
                        run.owner.clone(),
                        Action::Cancel {
                            agent_id: run.conversation.session_id.clone(),
                        },
                    )
                    .await?;
                return bots
                    .progress(
                        run.turn.id,
                        TurnStatus::Failed,
                        None,
                        Some("Provider permission request could not be denied".into()),
                    )
                    .await;
            }
            // Observe a fresh status after answering, even if this snapshot was already idle.
            tokio::select! {
                () = run.cancel.cancelled() => return cancel(bots, executor, run).await,
                () = tokio::time::sleep(Duration::from_millis(250)) => {}
            }
            continue;
        }
        match status["status"].as_str() {
            Some("idle") => {
                let reply = status["lastMessage"]
                    .as_str()
                    .filter(|text| text.len() <= 256 * 1024)
                    .ok_or(model::ErrorCode::BotIo)?
                    .to_owned();
                return bots
                    .progress(run.turn.id, TurnStatus::Completed, Some(reply), None)
                    .await;
            }
            // The turn was marked running before polling began.
            Some("running") => {}
            Some("error") => {
                return bots
                    .progress(
                        run.turn.id,
                        TurnStatus::Failed,
                        None,
                        Some("Provider execution failed".into()),
                    )
                    .await;
            }
            Some(_) | None => return Err(model::ErrorCode::BotIo),
        }
        tokio::select! {
            () = run.cancel.cancelled() => return cancel(bots, executor, run).await,
            () = tokio::time::sleep(Duration::from_millis(250)) => {}
        }
    }
}

/// Open the turn's native session, rotating the conversation once when the existing session
/// cannot reach the Bot's safe profile. `None` when the turn was settled instead.
async fn prepare(
    bots: &Bots,
    executor: &Arc<dyn Executor>,
    run: &mut Run,
) -> Result<Option<Created>, model::ErrorCode> {
    let Some(created) = open(executor, run).await? else {
        cancel(bots, executor, run).await?;
        return Ok(None);
    };
    if !created.reset_required {
        return Ok(Some(created));
    }
    let renewed = bots.rotate(run.conversation.id, run.turn.id).await?;
    run.turn.generation = renewed.generation;
    run.config = renewed.execution.clone();
    run.conversation = renewed;
    let Some(retried) = open(executor, run).await? else {
        cancel(bots, executor, run).await?;
        return Ok(None);
    };
    if retried.reset_required {
        let failure = Some(UNSAFE_SESSION.to_owned());
        bots.progress(run.turn.id, TurnStatus::Failed, None, failure)
            .await?;
        return Ok(None);
    }
    Ok(Some(retried))
}

/// Create or reuse the conversation's native session; `None` when the turn was cancelled.
async fn open(
    executor: &Arc<dyn Executor>,
    run: &Run,
) -> Result<Option<Created>, model::ErrorCode> {
    let create = executor.execute(
        run.owner.clone(),
        Action::Create {
            agent_id: run.conversation.session_id.clone(),
            config: run.config.clone(),
            instructions: run.instructions.clone(),
        },
    );
    tokio::select! {
        () = run.cancel.cancelled() => Ok(None),
        result = create => serde_json::from_value(result?)
            .map(Some)
            .map_err(|_| model::ErrorCode::BotIo),
    }
}

async fn deny_permissions(
    executor: &Arc<dyn Executor>,
    run: &Run,
    permissions: &[serde_json::Value],
) -> Result<(), model::ErrorCode> {
    for permission in permissions {
        let id = permission["id"].as_str().ok_or(model::ErrorCode::BotIo)?;
        executor
            .execute(
                run.owner.clone(),
                Action::Permission {
                    agent_id: run.conversation.session_id.clone(),
                    permission_id: id.to_owned(),
                    response: serde_json::json!({"behavior":"deny"}),
                },
            )
            .await?;
    }
    Ok(())
}

async fn cancel(
    bots: &Bots,
    executor: &Arc<dyn Executor>,
    run: &Run,
) -> Result<(), model::ErrorCode> {
    executor
        .execute(
            run.owner.clone(),
            Action::Cancel {
                agent_id: run.conversation.session_id.clone(),
            },
        )
        .await?;
    bots.progress(run.turn.id, TurnStatus::Cancelled, None, None)
        .await
}
