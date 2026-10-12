use super::*;
use crate::service::bot_execution::Action;
use domain::bot::{ExecutionConfig, SessionOwner};
use domain::{BotId, ConversationId};

#[tokio::test]
async fn bot_session_is_hidden_from_birth_owner_checked_and_idempotent() {
    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    let owner = SessionOwner {
        bot_id: BotId::generate(),
        conversation_id: ConversationId::generate(),
        workspace_id: "wks_0123456789abcdef".into(),
    };
    let id = uuid::Uuid::new_v4().to_string();
    let create = Action::Create {
        agent_id: id.clone(),
        config: ExecutionConfig {
            provider: "codex".into(),
            model: None,
            mode_id: None,
            thinking: None,
        },
        instructions: "Bot test".into(),
    };
    execution.bot(&owner, create.clone()).await.unwrap();
    let record = registry.get(&id).unwrap().unwrap();
    assert!(record.internal);
    assert_eq!(record.owner, Some(json!(owner)));
    assert_eq!(
        execution.bot(&owner, create).await.unwrap()["created"],
        false
    );
    assert_public_operations_hide(&execution, &fixture, &record).await;
    assert_eq!(registry.get(&id).unwrap().unwrap(), record);
    let mut intruder = owner.clone();
    intruder.bot_id = BotId::generate();
    assert_eq!(
        execution
            .bot(
                &intruder,
                Action::Send {
                    agent_id: id.clone(),
                    message_id: "message-1".into(),
                    text: "intrusion".into()
                }
            )
            .await,
        Err(model::ErrorCode::InvalidMessage)
    );
    assert_execution_and_close(&execution, &fixture, &owner, &id, &registry).await;
    execution.shutdown().await.unwrap();
}

async fn assert_public_operations_hide(
    execution: &AgentExecution,
    fixture: &Fixture,
    record: &domain::agent_runtime::PersistedAgentRuntimeRecord,
) {
    let hidden = execution
        .execute("agent.get.request", json!({"agentId":record.id}))
        .await
        .unwrap();
    assert!(hidden["agent"].is_null());
    assert!(hidden["error"].is_string());
    let listed = execution
        .execute("agent.list.request", json!({}))
        .await
        .unwrap();
    assert!(!listed.to_string().contains(&record.id));
    assert_eq!(
        execution
            .execute(
                "agent.create.request",
                json!({"agentId":record.id,"config":{"provider":"codex","cwd":fixture.cwd}})
            )
            .await,
        Err(ErrorCode::AgentNotFound)
    );
    assert_eq!(
        execution
            .execute("agent.resume.request", json!({"handle":record.persistence}))
            .await,
        Err(ErrorCode::AgentNotFound)
    );
}

async fn assert_execution_and_close(
    execution: &AgentExecution,
    fixture: &Fixture,
    owner: &SessionOwner,
    id: &str,
    registry: &FileBackedAgentRuntimeRegistry,
) {
    let send = Action::Send {
        agent_id: id.to_owned(),
        message_id: "message-1".into(),
        text: "hello".into(),
    };
    execution.bot(owner, send.clone()).await.unwrap();
    execution.bot(owner, send).await.unwrap();
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let value = execution
                .bot(
                    owner,
                    Action::Status {
                        agent_id: id.to_owned(),
                    },
                )
                .await
                .unwrap();
            if value["status"] == "idle" {
                break value;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(result["lastMessage"], "Echo: hello");
    assert_eq!(
        fixture
            .requests()
            .iter()
            .filter(|request| request["method"] == "turn/start")
            .count(),
        1
    );
    assert_release_and_resume(execution, owner, id, registry).await;
    execution
        .bot(
            owner,
            Action::Close {
                agent_id: id.to_owned(),
            },
        )
        .await
        .unwrap();
    assert!(registry.get(id).unwrap().unwrap().archived_at.is_some());
    assert_eq!(
        execution
            .execute(
                "agent.create.request",
                json!({"agentId":id,"config":{"provider":"codex","cwd":fixture.cwd}})
            )
            .await,
        Err(ErrorCode::AgentNotFound)
    );
    assert!(
        execution
            .execute(
                "agent.resume.request",
                json!({"handle":registry.get(id).unwrap().unwrap().persistence})
            )
            .await
            .is_err()
    );
}

async fn assert_release_and_resume(
    execution: &AgentExecution,
    owner: &SessionOwner,
    id: &str,
    registry: &FileBackedAgentRuntimeRegistry,
) {
    execution
        .bot(
            owner,
            Action::Release {
                agent_id: id.into(),
            },
        )
        .await
        .unwrap();
    assert!(registry.get(id).unwrap().unwrap().archived_at.is_none());
    execution
        .bot(
            owner,
            Action::Create {
                agent_id: id.into(),
                config: ExecutionConfig {
                    provider: "codex".into(),
                    model: None,
                    mode_id: None,
                    thinking: None,
                },
                instructions: "Updated workspace memory".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        registry
            .get(id)
            .unwrap()
            .unwrap()
            .config
            .unwrap()
            .system_prompt
            .as_deref(),
        Some("Updated workspace memory")
    );
    execution
        .bot(
            owner,
            Action::Send {
                agent_id: id.into(),
                message_id: "message-2".into(),
                text: "resumed".into(),
            },
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let status = execution
                .bot(
                    owner,
                    Action::Status {
                        agent_id: id.into(),
                    },
                )
                .await
                .unwrap();
            if status["status"] == "idle" {
                assert_eq!(status["lastMessage"], "Echo: resumed");
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}
