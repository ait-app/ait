use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::{Json, Router, routing::post};
use domain::bot::{TurnMessage, TurnMessageStatus};
use model::bot::Store;
use tokio::sync::Notify;

use super::*;
use crate::test_support::{Fixture, WORKSPACE, setup};

async fn registered_channel(fixture: &Fixture, bots: &Bots) -> Channel {
    bots.execute("bot.setup.request", setup(WORKSPACE))
        .await
        .unwrap();
    let value = bots
        .execute(
            "bot.channel.create.request",
            json!({
                "workspaceId":WORKSPACE,"channelId":ChannelId::generate(),"enabled":true,
                "config":{"kind":"telegram","tokenRef":"env:AIT_SERVER_CREDENTIAL_TEST",
                    "allowedSenders":["1"],"allowGroups":true}
            }),
        )
        .await
        .unwrap();
    fixture.channel(&value["channel"])
}

fn reaction_route(reactions: Arc<AtomicUsize>) -> axum::routing::MethodRouter {
    post(move |Json(body): Json<Value>| {
        let reactions = reactions.clone();
        async move {
            assert_eq!(body["chat_id"], "-100");
            assert!(body["message_id"] == 42 || body["message_id"] == 44);
            assert_eq!(
                body["reaction"],
                json!([{"type":"emoji","emoji":"\u{1f440}"}])
            );
            assert_eq!(body["is_big"], false);
            reactions.fetch_add(1, Ordering::SeqCst);
            Json(json!({"ok":true,"result":true}))
        }
    })
}

fn identity_route() -> axum::routing::MethodRouter {
    post(|| async {
        Json(json!({"ok":true,"result":{
            "id":2,"username":"ait_bot","can_read_all_group_messages":false
        }}))
    })
}

/// The privacy warning reaches the channel status a client reads.
///
/// The channel is disabled, so the supervisor runs no transport for it and nothing else writes
/// its status; an enabled channel is rewritten by the supervisor's own reconnect attempts.
#[tokio::test]
async fn telegram_publishes_the_group_privacy_warning_to_the_channel_status() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    bots.execute("bot.setup.request", setup(WORKSPACE))
        .await
        .unwrap();
    let id = ChannelId::generate();
    bots.execute(
        "bot.channel.create.request",
        json!({
            "workspaceId":WORKSPACE,"channelId":id,"enabled":false,
            "config":{"kind":"telegram","tokenRef":"env:AIT_SERVER_CREDENTIAL_TEST",
                "allowedSenders":["1"],"allowGroups":true}
        }),
    )
    .await
    .unwrap();
    let channel = fixture
        .store
        .catalog()
        .unwrap()
        .channels
        .into_iter()
        .find(|entry| entry.id == id)
        .unwrap();
    let app = Router::new()
        .route("/botfake/getMe", identity_route())
        .route(
            "/botfake/getWebhookInfo",
            post(|| async { Json(json!({"ok":true,"result":{"url":""}})) }),
        )
        .route(
            "/botfake/getUpdates",
            post(|| async { Json(json!({"ok":false,"error_code":400})) }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let cancel = CancellationToken::new();
    // The disabled channel refuses delivery, so the run ends once its status is published.
    run_at(
        &bots,
        &channel,
        (
            &client,
            &SecretString::from("fake".to_owned()),
            &format!("http://{address}"),
        ),
        &cancel,
    )
    .await
    .unwrap_err();
    let status = bots
        .execute(
            "bot.channel.status.request",
            json!({"workspaceId":WORKSPACE,"channelId":id}),
        )
        .await
        .unwrap();
    assert_eq!(status["connection"]["state"], "connected");
    assert_eq!(status["connection"]["error"], GROUP_PRIVACY_WARNING);
    bots.shutdown().await.unwrap();
    server.abort();
}

#[tokio::test]
async fn telegram_lifecycle_authenticates_polls_and_delivers_to_the_original_thread() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    let channel = registered_channel(&fixture, &bots).await;
    let delivered = Arc::new(Notify::new());
    let observed = delivered.clone();
    let reactions = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route("/botfake/getMe", identity_route())
        .route(
            "/botfake/setMessageReaction",
            reaction_route(reactions.clone()),
        )
        .route(
            "/botfake/getWebhookInfo",
            post(|| async { Json(json!({"ok":true,"result":{"url":""}})) }),
        )
        .route(
            "/botfake/getUpdates",
            post(|Json(body): Json<Value>| async move {
                if body["offset"] == 0 {
                    let mut message = update();
                    message["message"]["message_id"] = json!(42);
                    let mut follow_up = update();
                    follow_up["update_id"] = json!(11);
                    follow_up["message"]["message_id"] = json!(44);
                    follow_up["message"]["text"] = json!("@ait_bot follow up");
                    Json(json!({"ok":true,"result":[message,follow_up]}))
                } else {
                    assert_eq!(body["offset"], 12);
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    Json(json!({"ok":true,"result":[]}))
                }
            }),
        )
        .route(
            "/botfake/sendMessage",
            post(move |Json(body): Json<Value>| {
                let observed = observed.clone();
                async move {
                    assert_eq!(body["chat_id"], "-100");
                    assert_eq!(body["message_thread_id"], 3);
                    assert!(
                        body["reply_parameters"]["message_id"] == 42
                            || body["reply_parameters"]["message_id"] == 44
                    );
                    assert!(
                        body["text"] == "reply:@ait_bot hello"
                            || body["text"] == "reply:@ait_bot follow up"
                    );
                    observed.notify_one();
                    Json(json!({"ok":true,"result":{"message_id":43}}))
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let token = SecretString::from("fake".to_owned());
    let api = format!("http://{address}");
    let cancel = CancellationToken::new();
    let driver = run_at(&bots, &channel, (&client, &token, &api), &cancel);
    let verify = async {
        delivered.notified().await;
        loop {
            let conversations = fixture.store.conversations().unwrap();
            let turns = fixture.store.turns(conversations[0].id).unwrap();
            if turns.len() == 2
                && turns
                    .iter()
                    .all(|turn| turn.delivery == domain::bot::DeliveryStatus::Delivered)
            {
                assert!(turns.iter().any(|turn| {
                    turn.reply_to.as_deref() == Some("42")
                        && turn.reply.as_deref() == Some("reply:@ait_bot hello")
                }));
                assert!(turns.iter().any(|turn| {
                    turn.reply_to.as_deref() == Some("44")
                        && turn.reply.as_deref() == Some("reply:@ait_bot follow up")
                }));
                assert_eq!(reactions.load(Ordering::SeqCst), 2);
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        cancel.cancel();
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(driver, verify)
    })
    .await
    .unwrap();
    result.unwrap();
    bots.shutdown().await.unwrap();
    server.abort();
}

#[tokio::test]
async fn telegram_accumulates_multiple_assistant_messages_in_one_reply() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    let channel = registered_channel(&fixture, &bots).await;
    bots.accept(Incoming {
        channel_id: channel.id,
        peer: "-100".into(),
        sender: "1".into(),
        thread: Some("3".into()),
        message_id: "update-1".into(),
        text: "hello".into(),
        reply_to: Some("42".into()),
    })
    .await
    .unwrap();
    let conversation = fixture
        .store
        .conversations()
        .unwrap()
        .into_iter()
        .find(|conversation| conversation.peer == "-100")
        .unwrap();
    let mut turn = fixture.store.turns(conversation.id).unwrap().remove(0);
    turn.messages = vec![
        TurnMessage {
            id: "progress".into(),
            text: "checking".into(),
            status: TurnMessageStatus::Completed,
        },
        TurnMessage {
            id: "answer".into(),
            text: "**finished** with `code`".into(),
            status: TurnMessageStatus::Completed,
        },
    ];
    let app = Router::new()
        .route(
            "/botfake/sendMessage",
            post(|Json(body): Json<Value>| async move {
                assert_eq!(body["text"], "checking");
                assert_eq!(body["reply_parameters"]["message_id"], 42);
                Json(json!({"ok":true,"result":{"message_id":50}}))
            }),
        )
        .route(
            "/botfake/editMessageText",
            post(|Json(body): Json<Value>| async move {
                assert_eq!(
                    body,
                    json!({
                        "chat_id":"-100",
                        "message_id":50,
                        "text":"checking\n<b>finished</b> with <code>code</code>",
                        "parse_mode":"HTML"
                    })
                );
                Json(json!({"ok":true,"result":{"message_id":50}}))
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    send_reply(
        &reqwest::Client::builder().no_proxy().build().unwrap(),
        &SecretString::from("fake".to_owned()),
        &turn,
        &conversation,
        &format!("http://{address}"),
    )
    .await
    .unwrap();
    bots.shutdown().await.unwrap();
    server.abort();
}

#[tokio::test]
async fn existing_telegram_webhook_preserves_polling_cursor_and_waits_for_shutdown() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    let channel = registered_channel(&fixture, &bots).await;
    let webhook_checked = Arc::new(Notify::new());
    let observed = webhook_checked.clone();
    let app = Router::new()
        .route(
            "/botfake/getMe",
            post(|| async { Json(json!({"ok":true,"result":{"id":2,"username":"ait_bot"}})) }),
        )
        .route(
            "/botfake/getWebhookInfo",
            post(move || {
                let observed = observed.clone();
                async move {
                    observed.notify_one();
                    Json(json!({"ok":true,"result":{"url":"https://fixture.invalid/webhook"}}))
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let token = SecretString::from("fake".to_owned());
    let api = format!("http://{address}");
    let cancel = CancellationToken::new();
    let verify = async {
        webhook_checked.notified().await;
        cancel.cancel();
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(
            run_at(&bots, &channel, (&client, &token, &api), &cancel),
            verify
        )
    })
    .await
    .unwrap();
    result.unwrap();
    assert!(fixture.store.conversations().unwrap().is_empty());
    assert!(
        fixture.store.catalog().unwrap().channels[0]
            .cursor
            .is_none()
    );
    bots.shutdown().await.unwrap();
    server.abort();
}

#[tokio::test]
async fn telegram_rate_limit_retries_the_same_request_and_stops_after_three_attempts() {
    for succeed in [true, false] {
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let app = Router::new().route(
            "/botfake/getMe",
            post(move |Json(body): Json<Value>| {
                let observed = observed.clone();
                async move {
                    assert_eq!(body, json!({"fixture":"request"}));
                    let attempt = observed.fetch_add(1, Ordering::SeqCst);
                    if succeed && attempt == 2 {
                        Json(json!({"ok":true,"result":{"id":2}}))
                    } else {
                        Json(json!({"ok":false,"error_code":429,"parameters":{"retry_after":0}}))
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let result = call_at(
            &reqwest::Client::builder().no_proxy().build().unwrap(),
            &SecretString::from("fake".to_owned()),
            "getMe",
            json!({"fixture":"request"}),
            &format!("http://{address}"),
        )
        .await;
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        if succeed {
            assert_eq!(result.unwrap(), json!({"id":2}));
        } else {
            assert_eq!(result, Err(ErrorCode::BotIo));
        }
        server.abort();
    }
}
