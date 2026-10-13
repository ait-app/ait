use std::sync::atomic::Ordering;
use std::time::Duration;

use domain::BotId;
use model::ErrorCode;
use serde_json::{Value, json};

use crate::service::Bots;
use crate::test_support::{Fixture, creation};

#[tokio::test]
async fn failed_permission_denial_cancels_native_execution_and_fails_the_turn() {
    let fixture = Fixture::new();
    fixture.native.approval.store(true, Ordering::SeqCst);
    fixture.native.fail_denial.store(true, Ordering::SeqCst);
    let bots = fixture.bots();
    let bot = BotId::generate();
    let conversation = setup(&bots, bot).await;
    let turn = bots.execute("bot.message.send.request", json!({
        "botId":bot,"conversationId":conversation,"messageId":"denial-failure","text":"request"
    })).await.unwrap()["turn"]["id"].clone();
    let failed = wait(&bots, bot, &turn, "failed").await;
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

async fn setup(bots: &Bots, id: BotId) -> Value {
    bots.execute("bot.create.request", creation(id))
        .await
        .unwrap();
    bots.execute("bot.conversation.open.request", json!({"botId":id}))
        .await
        .unwrap()["conversation"]["id"]
        .clone()
}
async fn wait(bots: &Bots, bot: BotId, turn: &Value, status: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let value = bots
                .execute("bot.turn.get.request", json!({"botId":bot,"turnId":turn}))
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
    let first = BotId::generate();
    let second = BotId::generate();
    let conversation = setup(&bots, first).await;
    let other = setup(&bots, second).await;
    assert_eq!(
        bots.execute("bot.conversation.open.request", json!({"botId":first}))
            .await
            .unwrap()["conversation"]["id"],
        conversation
    );
    let input =
        json!({"botId":first,"conversationId":conversation,"messageId":"stable-1","text":"hello"});
    let accepted = bots
        .execute("bot.message.send.request", input.clone())
        .await
        .unwrap();
    let id = accepted["turn"]["id"].clone();
    let completed = wait(&bots, first, &id, "completed").await;
    assert_eq!(completed["turn"]["reply"], "reply:hello");
    assert_eq!(completed["turn"]["delivery"], "delivered");
    assert_eq!(
        bots.execute("bot.message.send.request", input)
            .await
            .unwrap()["turn"]["id"],
        id
    );
    assert_eq!(bots.execute("bot.message.send.request", json!({"botId":first,"conversationId":conversation,"messageId":"stable-1","text":"changed"})).await, Err(ErrorCode::IdempotencyConflict));
    assert_eq!(
        bots.execute(
            "bot.history.get.request",
            json!({"botId":second,"conversationId":conversation})
        )
        .await,
        Err(ErrorCode::BotNotFound)
    );
    assert_eq!(
        bots.execute("bot.turn.get.request", json!({"botId":second,"turnId":id}))
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
                json!({"botId":first,"conversationId":conversation})
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
            .execute(
                "bot.list.request",
                json!({"workspaceId":"wks_0123456789abcdef"})
            )
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
    let bot = BotId::generate();
    let conversation = setup(&bots, bot).await;
    let first = bots
        .execute(
            "bot.message.send.request",
            json!({"botId":bot,"conversationId":conversation,"messageId":"1","text":"first"}),
        )
        .await
        .unwrap()["turn"]["id"]
        .clone();
    wait(&bots, bot, &first, "running").await;
    assert_eq!(
        bots.execute("bot.permission.resolve.request", json!({"botId":bot,"turnId":first,"permissionId":"approval-1","response":{"behavior":"allow"}})).await,
        Err(ErrorCode::InvalidMessage),
    );
    let queued = bots
        .execute(
            "bot.message.send.request",
            json!({"botId":bot,"conversationId":conversation,"messageId":"2","text":"queued"}),
        )
        .await
        .unwrap()["turn"]["id"]
        .clone();
    bots.execute(
        "bot.turn.cancel.request",
        json!({"botId":bot,"turnId":queued}),
    )
    .await
    .unwrap();
    wait(&bots, bot, &queued, "cancelled").await;
    fixture.native.hold.store(false, Ordering::SeqCst);
    let completed = wait(&bots, bot, &first, "completed").await;
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
        .execute("bot.memory.get.request", json!({"botId":bot}))
        .await
        .unwrap();
    let changed = bots
        .execute(
            "bot.memory.update.request",
            json!({"botId":bot,"text":"durable knowledge","expected":memory["revision"]}),
        )
        .await
        .unwrap();
    assert_ne!(changed["revision"], memory["revision"]);
    assert_eq!(
        bots.execute(
            "bot.memory.update.request",
            json!({"botId":bot,"text":"overwrite","expected":memory["revision"]})
        )
        .await,
        Err(ErrorCode::BotConflict)
    );
    bots.execute(
        "bot.conversation.reset.request",
        json!({"botId":bot,"conversationId":conversation}),
    )
    .await
    .unwrap();
    bots.retire(&["wks_0123456789abcdef".into()]).await.unwrap();
    assert_eq!(
        bots.execute(
            "bot.message.send.request",
            json!({"botId":bot,"conversationId":conversation,"messageId":"3","text":"stopped"})
        )
        .await,
        Err(ErrorCode::BotConflict)
    );
    bots.shutdown().await.unwrap();
}
