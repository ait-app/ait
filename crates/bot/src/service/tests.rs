use std::sync::atomic::Ordering;
use std::time::Duration;

use model::ErrorCode;
use model::bot::Store;
use serde_json::{Value, json};

use crate::service::Bots;
use crate::test_support::{Fixture, OTHER_WORKSPACE, WORKSPACE, setup};

#[tokio::test]
async fn provider_failures_are_durable_and_ambiguous_statuses_require_explicit_recovery() {
    for (status, expected) in [
        (json!({"status":"error"}), "failed"),
        (json!({"status":"unknown"}), "uncertain"),
        (json!({"status":"idle"}), "uncertain"),
    ] {
        let fixture = Fixture::new();
        *fixture.native.status.lock().unwrap() = Some(status);
        let bots = fixture.bots();
        let conversation = open(&bots, WORKSPACE).await;
        let turn = bots
            .execute(
                "bot.message.send.request",
                json!({"workspaceId":WORKSPACE,
            "conversationId":conversation,"messageId":"failure","text":"fixture"}),
            )
            .await
            .unwrap()["turn"]["id"]
            .clone();

        let settled = wait(&bots, WORKSPACE, &turn, expected).await;

        assert!(settled["turn"]["reply"].is_null());
        assert!(settled["turn"]["error"].as_str().is_some());
        bots.shutdown().await.unwrap();
        let recovered = fixture.bots();
        let persisted = recovered
            .execute(
                "bot.turn.get.request",
                json!({"workspaceId":WORKSPACE,"turnId":turn}),
            )
            .await
            .unwrap();
        assert_eq!(persisted["turn"]["status"], expected);
        recovered.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn failed_permission_denial_cancels_native_execution_and_fails_the_turn() {
    let fixture = Fixture::new();
    fixture.native.approval.store(true, Ordering::SeqCst);
    fixture.native.fail_denial.store(true, Ordering::SeqCst);
    let bots = fixture.bots();
    let conversation = open(&bots, WORKSPACE).await;
    let turn = bots.execute("bot.message.send.request", json!({
        "workspaceId":WORKSPACE,"conversationId":conversation,"messageId":"denial-failure","text":"request"
    })).await.unwrap()["turn"]["id"].clone();
    let failed = wait(&bots, WORKSPACE, &turn, "failed").await;
    assert_eq!(failed["turn"]["pendingPermissions"], json!([]));
    assert!(
        fixture
            .native
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|(_, action)| {
                matches!(
                    action,
                    provider::service::bot_execution::Action::Cancel { .. }
                )
            })
    );
    bots.shutdown().await.unwrap();
}

async fn open(bots: &Bots, workspace: &str) -> Value {
    bots.execute("bot.setup.request", setup(workspace))
        .await
        .unwrap();
    bots.execute(
        "bot.conversation.open.request",
        json!({"workspaceId":workspace}),
    )
    .await
    .unwrap()["conversation"]["id"]
        .clone()
}
async fn wait(bots: &Bots, workspace: &str, turn: &Value, status: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let value = bots
                .execute(
                    "bot.turn.get.request",
                    json!({"workspaceId":workspace,"turnId":turn}),
                )
                .await
                .unwrap();
            if value["turn"]["status"] == status {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn builtin_turns_are_durable_idempotent_isolated_and_recoverable() {
    let fixture = Fixture::new();
    assert!(fixture.root.path().is_dir());
    let bots = fixture.bots();
    let conversation = open(&bots, WORKSPACE).await;
    let other = open(&bots, OTHER_WORKSPACE).await;
    assert_eq!(
        bots.execute(
            "bot.conversation.open.request",
            json!({"workspaceId":WORKSPACE})
        )
        .await
        .unwrap()["conversation"]["id"],
        conversation
    );
    let input = json!({"workspaceId":WORKSPACE,"conversationId":conversation,"messageId":"stable-1","text":"hello"});
    let accepted = bots
        .execute("bot.message.send.request", input.clone())
        .await
        .unwrap();
    let id = accepted["turn"]["id"].clone();
    let completed = wait(&bots, WORKSPACE, &id, "completed").await;
    assert_eq!(completed["turn"]["reply"], "reply:hello");
    assert_eq!(completed["turn"]["delivery"], "delivered");
    assert_eq!(
        bots.execute("bot.message.send.request", input)
            .await
            .unwrap()["turn"]["id"],
        id
    );
    assert_eq!(bots.execute("bot.message.send.request", json!({"workspaceId":WORKSPACE,"conversationId":conversation,"messageId":"stable-1","text":"changed"})).await, Err(ErrorCode::IdempotencyConflict));
    assert_eq!(
        bots.execute(
            "bot.history.get.request",
            json!({"workspaceId":OTHER_WORKSPACE,"conversationId":conversation})
        )
        .await,
        Err(ErrorCode::BotNotFound)
    );
    assert_eq!(
        bots.execute(
            "bot.turn.get.request",
            json!({"workspaceId":OTHER_WORKSPACE,"turnId":id})
        )
        .await,
        Err(ErrorCode::BotNotFound)
    );
    assert_ne!(conversation, other);
    bots.shutdown().await.unwrap();
    let restarted = fixture.bots();
    assert_eq!(
        restarted
            .execute(
                "bot.history.get.request",
                json!({"workspaceId":WORKSPACE,"conversationId":conversation})
            )
            .await
            .unwrap()["turns"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        fixture
            .native
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, action)| matches!(
                action,
                provider::service::bot_execution::Action::Send { .. }
            ))
            .count(),
        1
    );
    restarted.shutdown().await.unwrap();
    assert_eq!(
        restarted
            .execute("bot.get.request", json!({"workspaceId":WORKSPACE}))
            .await,
        Err(ErrorCode::ServerDraining)
    );
}

#[tokio::test]
async fn automatic_denial_queue_cancellation_memory_and_workspace_retirement() {
    let fixture = Fixture::new();
    fixture.native.approval.store(true, Ordering::SeqCst);
    fixture.native.hold.store(true, Ordering::SeqCst);
    let bots = fixture.bots();
    let conversation = open(&bots, WORKSPACE).await;
    let first = bots
        .execute(
            "bot.message.send.request",
            json!({"workspaceId":WORKSPACE,"conversationId":conversation,"messageId":"1","text":"first"}),
        )
        .await
        .unwrap()["turn"]["id"]
        .clone();
    wait(&bots, WORKSPACE, &first, "running").await;
    let queued = bots
        .execute(
            "bot.message.send.request",
            json!({"workspaceId":WORKSPACE,"conversationId":conversation,"messageId":"2","text":"queued"}),
        )
        .await
        .unwrap()["turn"]["id"]
        .clone();
    bots.execute(
        "bot.turn.cancel.request",
        json!({"workspaceId":WORKSPACE,"turnId":queued}),
    )
    .await
    .unwrap();
    wait(&bots, WORKSPACE, &queued, "cancelled").await;
    fixture.native.hold.store(false, Ordering::SeqCst);
    let completed = wait(&bots, WORKSPACE, &first, "completed").await;
    assert_eq!(completed["turn"]["pendingPermissions"], json!([]));
    let responses: Vec<_> = fixture
        .native
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter_map(|(_, action)| {
            if let provider::service::bot_execution::Action::Permission { response, .. } = action {
                Some(response.clone())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(responses, [json!({"behavior":"deny"})]);
    let memory = bots
        .execute("bot.memory.get.request", json!({"workspaceId":WORKSPACE}))
        .await
        .unwrap();
    let changed = bots
        .execute(
            "bot.memory.update.request",
            json!({"workspaceId":WORKSPACE,"text":"durable knowledge","expected":memory["revision"]}),
        )
        .await
        .unwrap();
    assert_ne!(changed["revision"], memory["revision"]);
    assert_eq!(
        bots.execute(
            "bot.memory.update.request",
            json!({"workspaceId":WORKSPACE,"text":"overwrite","expected":memory["revision"]})
        )
        .await,
        Err(ErrorCode::BotConflict)
    );
    bots.execute(
        "bot.conversation.reset.request",
        json!({"workspaceId":WORKSPACE,"conversationId":conversation}),
    )
    .await
    .unwrap();
    bots.retire(&[WORKSPACE.into()]).await.unwrap();
    assert_eq!(fixture.store.memory(WORKSPACE).unwrap(), "");
    assert_eq!(
        bots.execute(
            "bot.message.send.request",
            json!({"workspaceId":WORKSPACE,"conversationId":conversation,"messageId":"3","text":"stopped"})
        )
        .await,
        Err(ErrorCode::BotNotFound)
    );
    bots.shutdown().await.unwrap();
}
