use std::sync::atomic::{AtomicUsize, Ordering};

use axum::{
    Json, Router,
    routing::{get, post},
};
use domain::{BotId, ChannelId};
use model::bot::Store;

use super::super::*;
use crate::test_support::{Fixture, creation};

#[tokio::test]
async fn tenant_token_is_cached_and_native_reply_uses_the_original_chat() {
    let count = Arc::new(AtomicUsize::new(0));
    let token_count = count.clone();
    let app = Router::new()
        .route(
            "/open-apis/auth/v3/tenant_access_token/internal",
            post(move |Json(body): Json<Value>| {
                let count = token_count.clone();
                async move {
                    assert_eq!(body["app_id"], "app-1");
                    count.fetch_add(1, Ordering::SeqCst);
                    Json(json!({"code":0,"tenant_access_token":"offline-token","expire":7200}))
                }
            }),
        )
        .route(
            "/open-apis/bot/v3/info",
            get(|headers: axum::http::HeaderMap| async move {
                assert_eq!(headers["authorization"], "Bearer offline-token");
                Json(json!({"code":0,"bot":{"open_id":"ou_bot"}}))
            }),
        )
        .route(
            "/open-apis/im/v1/messages",
            post(|Json(body): Json<Value>| async move {
                assert_eq!(body["receive_id"], "oc_original");
                assert_eq!(body["msg_type"], "text");
                let content: Value =
                    serde_json::from_str(body["content"].as_str().unwrap()).unwrap();
                assert_eq!(content["text"], "reply");
                assert!(body["uuid"].as_str().unwrap().ends_with("-0"));
                Json(json!({"code":0,"data":{"message_id":"om_reply"}}))
            }),
        )
        .route(
            "/open-apis/im/v1/messages/om_original/reply",
            post(|Json(body): Json<Value>| async move {
                assert!(body.get("receive_id").is_none());
                assert_eq!(body["reply_in_thread"], true);
                assert_eq!(body["msg_type"], "text");
                Json(json!({"code":0,"data":{"message_id":"om_reply"}}))
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let bot = BotId::generate();
    let id = ChannelId::generate();
    state.execute("bot.create.request", creation(bot)).unwrap();
    let value = state.execute("bot.channel.create.request",json!({"botId":bot,"channelId":id,"enabled":true,"config":{"kind":"feishu","appId":"app-1","secretRef":"env:AIT_SERVER_CREDENTIAL_TEST","allowedSenders":["ou_alice"]}})).unwrap();
    let channel: Channel = serde_json::from_value(value["channel"].clone()).unwrap();
    let input = state
        .accept(Incoming {
            channel_id: id,
            peer: "oc_original".into(),
            sender: "ou_alice".into(),
            thread: None,
            message_id: "event-1".into(),
            text: "hello".into(),
            reply_to: None,
        })
        .unwrap();
    let turn_id = serde_json::from_value(input["turn"]["id"].clone()).unwrap();
    let mut turn = state.turn(turn_id).unwrap();
    turn.reply = Some("reply".into());
    let conversation = fixture.store.conversations().unwrap().remove(0);
    let api = format!("http://{address}");
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let secret = SecretString::from("offline-secret".to_owned());
    let connection = Connection {
        channel: &channel,
        client: &client,
        app_id: "app-1",
        secret: &secret,
        token: Mutex::new(None),
        api: &api,
    };
    assert_eq!(
        connection
            .call("GET", "/open-apis/bot/v3/info", None)
            .await
            .unwrap()["bot"]["open_id"],
        "ou_bot"
    );
    connection
        .reply(turn.clone(), conversation.clone())
        .await
        .unwrap();
    turn.reply_to = Some("om_original".into());
    let mut threaded = conversation;
    threaded.thread = Some("om_root".into());
    connection.reply(turn, threaded).await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    connection.token.lock().await.as_mut().unwrap().expires = Instant::now();
    connection.token().await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 2);
    server.abort();
}

#[tokio::test]
async fn websocket_events_are_acknowledged_after_durable_admission_and_deduplicated() {
    let fixture = Fixture::new();
    fixture.native.hold.store(true, Ordering::SeqCst);
    let bots = fixture.bots();
    let bot = BotId::generate();
    let id = ChannelId::generate();
    bots.execute("bot.create.request", creation(bot))
        .await
        .unwrap();
    let value = bots.execute("bot.channel.create.request",json!({"botId":bot,"channelId":id,"enabled":true,"config":{"kind":"feishu","appId":"app-1","secretRef":"env:AIT_SERVER_CREDENTIAL_TEST","allowedSenders":["ou_alice"]}})).await.unwrap();
    let channel: Channel = serde_json::from_value(value["channel"].clone()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let cancel = CancellationToken::new();
    let server_cancel = cancel.clone();
    let store = fixture.store.clone();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
        let event = json!({"header":{"event_type":"im.message.receive_v1","event_id":"event-1"},"event":{"sender":{"sender_type":"user","sender_id":{"open_id":"ou_alice"}},"message":{"chat_type":"p2p","chat_id":"oc_chat","message_type":"text","content":"{\"text\":\"hello\"}"}}});
        let frame = Frame {
            seq_id: 12,
            log_id: 99,
            service: 7,
            method: 1,
            headers: vec![Header {
                key: "type".into(),
                value: "event".into(),
            }],
            payload: Some(serde_json::to_vec(&event).unwrap()),
            ..Frame::default()
        };
        for _ in 0..2 {
            socket
                .send(WsMessage::Binary(frame.encode_to_vec().into()))
                .await
                .unwrap();
            loop {
                let WsMessage::Binary(bytes) = socket.next().await.unwrap().unwrap() else {
                    continue;
                };
                let reply = Frame::decode(bytes).unwrap();
                if reply.method != 1 {
                    continue;
                }
                assert_eq!(reply.seq_id, 12);
                assert_eq!(reply.log_id, 99);
                let result: Value =
                    serde_json::from_slice(reply.payload.as_ref().unwrap()).unwrap();
                assert_eq!(result["code"], 200);
                let conversations = store.conversations().unwrap();
                assert_eq!(store.turns(conversations[0].id).unwrap().len(), 1);
                break;
            }
        }
        server_cancel.cancel();
    });
    let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}"))
        .await
        .unwrap();
    let client = reqwest::Client::new();
    let secret = SecretString::from("offline".to_owned());
    let connection = Connection {
        channel: &channel,
        client: &client,
        app_id: "app-1",
        secret: &secret,
        token: Mutex::new(None),
        api: API,
    };
    tokio::time::timeout(
        Duration::from_secs(5),
        receive(
            &bots,
            &connection,
            socket,
            StreamOptions {
                service: 7,
                ping_interval: 30,
                bot_id: "ou_bot",
            },
            &cancel,
        ),
    )
    .await
    .unwrap()
    .unwrap();
    server.await.unwrap();
    bots.shutdown().await.unwrap();
}
