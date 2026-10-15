use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::{Json, Router, routing::post};
use model::bot::Store;
use tokio::sync::Notify;

use super::*;
use crate::test_support::{Fixture, creation};

async fn registered_channel(bots: &Bots) -> Channel {
    let bot = BotId::generate();
    bots.execute("bot.create.request", creation(bot))
        .await
        .unwrap();
    let value = bots
        .execute(
            "bot.channel.create.request",
            json!({
                "botId":bot,"channelId":ChannelId::generate(),"enabled":true,
                "config":{"kind":"telegram","tokenRef":"env:AIT_SERVER_CREDENTIAL_TEST",
                    "allowedSenders":["1"],"allowGroups":true}
            }),
        )
        .await
        .unwrap();
    serde_json::from_value(value["channel"].clone()).unwrap()
}

#[tokio::test]
async fn telegram_lifecycle_authenticates_polls_and_delivers_to_the_original_thread() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    let channel = registered_channel(&bots).await;
    let delivered = Arc::new(Notify::new());
    let observed = delivered.clone();
    let app = Router::new()
        .route(
            "/botfake/getMe",
            post(|| async { Json(json!({"ok":true,"result":{"id":2,"username":"ait_bot"}})) }),
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
                    Json(json!({"ok":true,"result":[message]}))
                } else {
                    assert_eq!(body["offset"], 11);
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
                    assert_eq!(body["reply_parameters"]["message_id"], 42);
                    assert_eq!(body["text"], "reply:@ait_bot hello");
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
            if turns[0].delivery == domain::bot::DeliveryStatus::Delivered {
                assert_eq!(turns[0].reply.as_deref(), Some("reply:@ait_bot hello"));
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
async fn existing_telegram_webhook_preserves_polling_cursor_and_waits_for_shutdown() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    let channel = registered_channel(&bots).await;
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
