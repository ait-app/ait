use std::sync::Arc;
use std::time::Duration;

use domain::bot::{Conversation, ExecutionConfig, SessionOwner, Turn, TurnStatus};
use provider::service::bot_execution::Action;
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

pub(crate) async fn run(bots: Bots, executor: Arc<dyn Executor>, run: Run) {
    let conversation = run.conversation.id;
    let result = perform(&bots, &executor, &run).await;
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
    run: &Run,
) -> Result<(), model::ErrorCode> {
    let create = executor.execute(
        run.owner.clone(),
        Action::Create {
            agent_id: run.conversation.session_id.clone(),
            config: run.config.clone(),
            instructions: run.instructions.clone(),
        },
    );
    tokio::select! {
        () = run.cancel.cancelled() => return cancel(bots, executor, run).await,
        result = create => { result?; }
    }
    let send = executor.execute(
        run.owner.clone(),
        Action::Send {
            agent_id: run.conversation.session_id.clone(),
            message_id: run.turn.id.to_string(),
            text: run.turn.text.clone(),
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
            Some("running") => {
                bots.progress(run.turn.id, TurnStatus::Running, None, None)
                    .await?;
            }
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
