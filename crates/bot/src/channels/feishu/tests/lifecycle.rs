use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json, Router,
    routing::{get, post},
};
use domain::{BotId, ChannelId};
use model::bot::Store;
use tokio::sync::Notify;

use super::super::*;
use crate::test_support::{Fixture, creation};

async fn registered_channel(bots: &Bots) -> Channel {
    let bot = BotId::generate();
    bots.execute("bot.create.request", creation(bot))
        .await
        .unwrap();
    let value = bots.execute("bot.channel.create.request", json!({
        "botId":bot,"channelId":ChannelId::generate(),"enabled":true,
        "config":{"kind":"feishu","appId":"app-1","secretRef":"env:AIT_SERVER_CREDENTIAL_TEST",
            "allowedSenders":["ou_alice"],"allowGroups":true}
    })).await.unwrap();
    serde_json::from_value(value["channel"].clone()).unwrap()
}

fn api(endpoint: Value, delivered: Arc<Notify>) -> Router {
    Router::new()
        .route(
            "/open-apis/auth/v3/tenant_access_token/internal",
            post(|| async {
                Json(json!({"code":0,"tenant_access_token":"offline-token","expire":7200}))
            }),
        )
        .route(
            "/open-apis/bot/v3/info",
            get(|| async { Json(json!({"code":0,"bot":{"open_id":"ou_bot"}})) }),
        )
        .route(
            "/callback/ws/endpoint",
            post(move |Json(body): Json<Value>| {
                let endpoint = endpoint.clone();
                async move {
                    assert_eq!(body, json!({"AppID":"app-1","AppSecret":"offline-secret"}));
                    Json(endpoint)
                }
            }),
        )
        .route(
            "/open-apis/im/v1/messages/om_original/reply",
            post(move |Json(body): Json<Value>| {
                let delivered = delivered.clone();
                async move {
                    assert_eq!(body["reply_in_thread"], false);
                    let content: Value =
                        serde_json::from_str(body["content"].as_str().unwrap()).unwrap();
                    assert_eq!(content["text"], "reply:hello");
                    delivered.notify_one();
                    Json(json!({"code":0,"data":{"message_id":"om_reply"}}))
                }
            }),
        )
}

#[tokio::test]
async fn feishu_lifecycle_authenticates_connects_and_delivers_a_durable_reply() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    let channel = registered_channel(&bots).await;
    let delivered = Arc::new(Notify::new());
    let app = api(
        json!({"code":0,"data":{"URL":"wss://fixture.feishu.cn/ws?service_id=7",
        "ClientConfig":{"PingInterval":30}}}),
        delivered.clone(),
    );
    let http = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let http_address = http.local_addr().unwrap();
    let http_task = tokio::spawn(async move {
        axum::serve(http, app).await.unwrap();
    });
    let ws = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ws_address = ws.local_addr().unwrap();
    let ws_task = tokio::spawn(async move {
        let (tcp, _) = ws.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
        let event = json!({"header":{"event_type":"im.message.receive_v1","event_id":"event-1"},
            "event":{"sender":{"sender_type":"user","sender_id":{"open_id":"ou_alice"}},
            "message":{"chat_type":"p2p","chat_id":"oc_chat","message_id":"om_original",
                "message_type":"text","content":"{\"text\":\"hello\"}"}}});
        let frame = Frame {
            method: 1,
            service: 7,
            headers: vec![Header {
                key: "type".into(),
                value: "event".into(),
            }],
            payload: Some(serde_json::to_vec(&event).unwrap()),
            ..Frame::default()
        };
        socket
            .send(WsMessage::Binary(frame.encode_to_vec().into()))
            .await
            .unwrap();
        while socket.next().await.is_some() {}
    });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let secret = SecretString::from("offline-secret".to_owned());
    let base = format!("http://{http_address}");
    let cancel = CancellationToken::new();
    let driver = run_at(
        &bots,
        &channel,
        (&client, &secret, &base),
        &cancel,
        move |endpoint| async move {
            assert_eq!(endpoint, "wss://fixture.feishu.cn/ws?service_id=7");
            tokio_tungstenite::connect_async(format!("ws://{ws_address}"))
                .await
                .map(|(socket, _)| socket)
                .map_err(|_| ErrorCode::BotIo)
        },
    );
    let verify = async {
        delivered.notified().await;
        loop {
            let conversations = fixture.store.conversations().unwrap();
            let turns = fixture.store.turns(conversations[0].id).unwrap();
            if turns[0].delivery == domain::bot::DeliveryStatus::Delivered {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        cancel.cancel();
    };
    let (result, ()) = Box::pin(tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(driver, verify)
    }))
    .await
    .unwrap();
    result.unwrap();
    bots.shutdown().await.unwrap();
    http_task.abort();
    ws_task.abort();
}

#[tokio::test]
async fn untrusted_feishu_endpoints_never_reach_the_websocket_dialer() {
    for endpoint in [
        json!({"code":1}),
        json!({"code":0}),
        json!({"code":0,"data":{}}),
        json!({"code":0,"data":{"URL":"invalid"}}),
        json!({"code":0,"data":{"URL":"ws://fixture.feishu.cn?service_id=7"}}),
        json!({"code":0,"data":{"URL":"wss://feishu.cn.evil.test?service_id=7"}}),
        json!({"code":0,"data":{"URL":"wss://fixture.feishu.cn"}}),
    ] {
        let fixture = Fixture::new();
        let bots = fixture.bots();
        let channel = registered_channel(&bots).await;
        let app = api(endpoint, Arc::new(Notify::new()));
        let http = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = http.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(http, app).await.unwrap();
        });
        let result = run_at(
            &bots,
            &channel,
            (
                &reqwest::Client::builder().no_proxy().build().unwrap(),
                &SecretString::from("offline-secret".to_owned()),
                &format!("http://{address}"),
            ),
            &CancellationToken::new(),
            |_| async { panic!("untrusted endpoint was dialed") },
        )
        .await;
        assert_eq!(result, Err(ErrorCode::BotIo));
        bots.shutdown().await.unwrap();
        task.abort();
    }
}

#[tokio::test]
async fn websocket_dialer_preserves_frames_with_the_production_size_limit() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
        socket
            .send(WsMessage::Text("fixture".into()))
            .await
            .unwrap();
        let _ = socket
            .send(WsMessage::Text("x".repeat(1024 * 1024 + 1).into()))
            .await;
    });
    let mut socket = connect(format!("ws://{address}")).await.unwrap();
    assert_eq!(
        socket.next().await.unwrap().unwrap(),
        WsMessage::Text("fixture".into())
    );
    assert!(socket.next().await.unwrap().is_err());
    drop(socket);
    task.await.unwrap();
}

#[tokio::test]
async fn feishu_adapter_rejects_builtin_channel_configuration_before_contacting_provider() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    let channel = Channel {
        id: ChannelId::generate(),
        bot_id: BotId::generate(),
        enabled: true,
        cursor: None,
        deleted: false,
        config: ChannelConfig::Builtin,
    };
    let result = run(
        &bots,
        &channel,
        &reqwest::Client::new(),
        &SecretString::from("fixture".to_owned()),
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(result, Err(ErrorCode::InvalidMessage));
    bots.shutdown().await.unwrap();
}
