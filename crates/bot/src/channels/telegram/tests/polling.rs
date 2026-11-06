use std::sync::atomic::Ordering;

use axum::{Json, Router, routing::post};
use model::bot::Store;

use super::*;
use crate::test_support::{Fixture, WORKSPACE, setup};

#[tokio::test]
async fn polling_acknowledges_only_after_input_and_cursor_commit() {
    let fixture = Fixture::new();
    fixture.native.hold.store(true, Ordering::SeqCst);
    let bots = fixture.bots();
    let id = ChannelId::generate();
    bots.execute("bot.setup.request", setup(WORKSPACE))
        .await
        .unwrap();
    let created = bots.execute("bot.channel.create.request",json!({"workspaceId":WORKSPACE,"channelId":id,"enabled":true,"config":{"kind":"telegram","tokenRef":"env:AIT_SERVER_CREDENTIAL_TEST","allowedSenders":["1"],"allowGroups":true}})).await.unwrap();
    let channel: Channel = fixture.channel(&created["channel"]);
    let cancel = CancellationToken::new();
    let server_cancel = cancel.clone();
    let store = fixture.store.clone();
    let app = Router::new().route(
        "/botfake/getUpdates",
        post(move |Json(body): Json<Value>| {
            let store = store.clone();
            let cancel = server_cancel.clone();
            async move {
                if body["offset"] == 0 {
                    Json(json!({"ok":true,"result":[update()]}))
                } else {
                    assert_eq!(body["offset"], 11);
                    assert_eq!(
                        store
                            .catalog()
                            .unwrap()
                            .channels
                            .iter()
                            .find(|channel| channel.id == id)
                            .unwrap()
                            .cursor,
                        Some(11)
                    );
                    let conversations = store.conversations().unwrap();
                    assert_eq!(store.turns(conversations[0].id).unwrap().len(), 1);
                    cancel.cancel();
                    Json(json!({"ok":true,"result":[]}))
                }
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
    let receiver = Receiver {
        bots: &bots,
        channel: &channel,
        client: &client,
        token: &token,
        identity: 2,
        username: "ait_bot",
        api: &api,
    };
    tokio::time::timeout(Duration::from_secs(5), receiver.receive(&cancel))
        .await
        .unwrap()
        .unwrap();
    bots.shutdown().await.unwrap();
    server.abort();
}

#[tokio::test]
async fn refused_update_is_skipped_without_blocking_later_updates() {
    let fixture = Fixture::new();
    fixture.native.hold.store(true, Ordering::SeqCst);
    let bots = fixture.bots();
    let id = ChannelId::generate();
    bots.execute("bot.setup.request", setup(WORKSPACE))
        .await
        .unwrap();
    let created = bots.execute("bot.channel.create.request",json!({"workspaceId":WORKSPACE,"channelId":id,"enabled":true,"config":{"kind":"telegram","tokenRef":"env:AIT_SERVER_CREDENTIAL_TEST","allowedSenders":["1"],"allowGroups":true}})).await.unwrap();
    let channel: Channel = fixture.channel(&created["channel"]);
    let cancel = CancellationToken::new();
    let server_cancel = cancel.clone();
    let store = fixture.store.clone();
    let app = Router::new().route(
        "/botfake/getUpdates",
        post(move |Json(body): Json<Value>| {
            let store = store.clone();
            let cancel = server_cancel.clone();
            async move {
                if body["offset"] == 0 {
                    let mut refused = update();
                    refused["message"]["text"] = json!("@ait_bot invalid\u{0}");
                    let mut accepted = update();
                    accepted["update_id"] = json!(11);
                    Json(json!({"ok":true,"result":[refused, accepted]}))
                } else {
                    assert_eq!(body["offset"], 12);
                    let conversations = store.conversations().unwrap();
                    let turns = store.turns(conversations[0].id).unwrap();
                    assert_eq!(turns.len(), 1);
                    assert_eq!(turns[0].message_id, "11");
                    cancel.cancel();
                    Json(json!({"ok":true,"result":[]}))
                }
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
    let receiver = Receiver {
        bots: &bots,
        channel: &channel,
        client: &client,
        token: &token,
        identity: 2,
        username: "ait_bot",
        api: &api,
    };
    tokio::time::timeout(Duration::from_secs(5), receiver.receive(&cancel))
        .await
        .unwrap()
        .unwrap();
    bots.shutdown().await.unwrap();
    server.abort();
}

#[tokio::test]
async fn saturated_conversation_drops_with_a_notice_without_blocking_other_chats() {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    let fixture = Fixture::new();
    fixture.native.hold.store(true, Ordering::SeqCst);
    let bots = fixture.bots();
    let id = ChannelId::generate();
    bots.execute("bot.setup.request", setup(WORKSPACE))
        .await
        .unwrap();
    let created = bots.execute("bot.channel.create.request",json!({"workspaceId":WORKSPACE,"channelId":id,"enabled":true,"config":{"kind":"telegram","tokenRef":"env:AIT_SERVER_CREDENTIAL_TEST","allowedSenders":["1"],"allowGroups":true}})).await.unwrap();
    let channel: Channel = fixture.channel(&created["channel"]);
    let cancel = CancellationToken::new();
    let server_cancel = cancel.clone();
    let store = fixture.store.clone();
    let notices = Arc::new(AtomicUsize::new(0));
    let counted = notices.clone();
    let app = Router::new()
        .route(
            "/botfake/getUpdates",
            post(move |Json(body): Json<Value>| {
                let store = store.clone();
                let cancel = server_cancel.clone();
                let notices = counted.clone();
                async move {
                    if body["offset"] == 0 {
                        let mut updates: Vec<Value> = (100..140)
                            .map(|index| {
                                let mut saturated = update();
                                saturated["update_id"] = json!(index);
                                saturated
                            })
                            .collect();
                        let mut other = update();
                        other["update_id"] = json!(140);
                        other["message"]["chat"]["id"] = json!(-200);
                        updates.push(other);
                        Json(json!({"ok":true,"result":updates}))
                    } else {
                        assert_eq!(body["offset"], 141);
                        let mut stored = 0;
                        let mut other = 0;
                        for conversation in store.conversations().unwrap() {
                            let turns = store.turns(conversation.id).unwrap().len();
                            if conversation.peer == "-200" {
                                other += turns;
                            } else {
                                stored += turns;
                            }
                        }
                        assert_eq!(other, 1);
                        let dropped = notices.load(Ordering::SeqCst);
                        assert!(dropped > 0);
                        assert_eq!(stored + dropped, 40);
                        cancel.cancel();
                        Json(json!({"ok":true,"result":[]}))
                    }
                }
            }),
        )
        .route(
            "/botfake/sendMessage",
            post(move |Json(body): Json<Value>| {
                let notices = notices.clone();
                async move {
                    assert_eq!(body["chat_id"], "-100");
                    assert_eq!(body["text"], BUSY_NOTICE);
                    assert_eq!(body["message_thread_id"], 3);
                    notices.fetch_add(1, Ordering::SeqCst);
                    Json(json!({"ok":true,"result":{}}))
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
    let receiver = Receiver {
        bots: &bots,
        channel: &channel,
        client: &client,
        token: &token,
        identity: 2,
        username: "ait_bot",
        api: &api,
    };
    tokio::time::timeout(Duration::from_secs(10), receiver.receive(&cancel))
        .await
        .unwrap()
        .unwrap();
    bots.shutdown().await.unwrap();
    server.abort();
}

#[tokio::test]
async fn transport_errors_do_not_return_token_bearing_urls() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route(
                "/botfake/getMe",
                post(|| async { Json(json!({"ok":false,"description":"secret: fake"})) }),
            ),
        )
        .await
        .unwrap();
    });
    let result = call_at(
        &reqwest::Client::builder().no_proxy().build().unwrap(),
        &SecretString::from("fake".to_owned()),
        "getMe",
        json!({}),
        &format!("http://{address}"),
    )
    .await;
    assert_eq!(result, Err(ErrorCode::BotIo));
    server.abort();
}
