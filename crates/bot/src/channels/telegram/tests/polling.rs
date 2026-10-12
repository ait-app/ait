use std::sync::atomic::Ordering;

use axum::{Json, Router, routing::post};
use model::bot::Store;

use super::*;
use crate::test_support::{Fixture, creation};

#[tokio::test]
async fn polling_acknowledges_only_after_input_and_cursor_commit() {
    let fixture = Fixture::new();
    fixture.native.hold.store(true, Ordering::SeqCst);
    let bots = fixture.bots();
    let bot = BotId::generate();
    let id = ChannelId::generate();
    bots.execute("bot.create.request", creation(bot))
        .await
        .unwrap();
    let created = bots.execute("bot.channel.create.request",json!({"botId":bot,"channelId":id,"enabled":true,"config":{"kind":"telegram","tokenRef":"env:AIT_SERVER_CREDENTIAL_TEST","allowedSenders":["1"],"allowGroups":true}})).await.unwrap();
    let channel: Channel = serde_json::from_value(created["channel"].clone()).unwrap();
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
    tokio::time::timeout(std::time::Duration::from_secs(5), receiver.receive(&cancel))
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
