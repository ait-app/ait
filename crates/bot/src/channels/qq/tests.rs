use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use axum::http::HeaderMap;
use axum::{
    Json, Router,
    routing::{get, post},
};
use domain::{BotId, ChannelId};
use model::bot::Store;
use tokio::sync::Notify;

use super::*;
use crate::test_support::{Fixture, WORKSPACE, setup};

fn channel(allow_groups: bool) -> Channel {
    Channel {
        id: ChannelId::generate(),
        bot_id: BotId::generate(),
        enabled: true,
        cursor: None,
        sync_token: None,
        deleted: false,
        config: ChannelConfig::Qq {
            app_id: "app-1".into(),
            secret_ref: "env:AIT_SERVER_CREDENTIAL_QQ".into(),
            allowed_senders: vec!["user-open".into()],
            allow_groups,
        },
    }
}

fn private_event() -> Value {
    json!({"id":"msg-1","content":" hello ","author":{"user_openid":"user-open"}})
}

fn group_event() -> Value {
    json!({"id":"msg-2","content":" <@!bot> hi there","group_openid":"group-1",
        "author":{"member_openid":"member-open"}})
}

#[test]
fn private_messages_route_to_the_sender() {
    let routed = inbound(&channel(false), PRIVATE_MESSAGE, &private_event())
        .unwrap()
        .unwrap();
    assert_eq!(routed.peer, "c2c:user-open");
    assert_eq!(routed.sender, "user-open");
    assert_eq!(routed.text, "hello");
    assert_eq!(routed.reply_to.as_deref(), Some("msg-1"));
}

#[test]
fn group_mentions_need_the_channel_to_allow_groups() {
    let routed = inbound(&channel(true), GROUP_MESSAGE, &group_event())
        .unwrap()
        .unwrap();
    assert_eq!(routed.peer, "group:group-1");
    assert_eq!(routed.sender, "member-open");
    assert_eq!(routed.text, "hi there");
    assert!(
        inbound(&channel(false), GROUP_MESSAGE, &group_event())
            .unwrap()
            .is_none()
    );
}

#[test]
fn other_events_and_empty_messages_are_ignored() {
    let channel = channel(true);
    assert!(
        inbound(&channel, "GUILD_AT_MESSAGE_CREATE", &private_event())
            .unwrap()
            .is_none()
    );
    let mut empty = private_event();
    empty["content"] = json!("   ");
    assert!(
        inbound(&channel, PRIVATE_MESSAGE, &empty)
            .unwrap()
            .is_none()
    );
    let mut anonymous = private_event();
    anonymous["author"] = json!({});
    assert!(
        inbound(&channel, PRIVATE_MESSAGE, &anonymous)
            .unwrap()
            .is_none()
    );
}

#[test]
fn leading_mentions_are_removed_in_either_form() {
    assert_eq!(strip_mention("<@!123> hello"), "hello");
    assert_eq!(strip_mention("@Ait  hello"), "hello");
    assert_eq!(strip_mention("  hello @Ait"), "hello @Ait");
    assert_eq!(strip_mention("@Ait"), "");
}

#[test]
fn stored_peers_map_only_to_qq_message_paths() {
    assert_eq!(
        message_path("c2c:abc-1").unwrap(),
        "/v2/users/abc-1/messages"
    );
    assert_eq!(
        message_path("group:G_1").unwrap(),
        "/v2/groups/G_1/messages"
    );
    for peer in ["abc", "c2c:", "other:abc", "c2c:../../gateway", "group:a b"] {
        assert_eq!(message_path(peer), Err(ErrorCode::BotIo), "{peer}");
    }
}

#[test]
fn only_secure_qq_gateways_are_dialed() {
    assert!(trusted_gateway("wss://api.sgroup.qq.com/websocket").is_ok());
    for endpoint in [
        "ws://api.sgroup.qq.com/websocket",
        "wss://qq.com.evil.example/websocket",
        "wss://evilqq.com/websocket",
        "nonsense",
    ] {
        assert_eq!(
            trusted_gateway(endpoint),
            Err(ErrorCode::BotIo),
            "{endpoint}"
        );
    }
}

/// Serves the token, gateway and message routes, answering sends from `replies` in order.
struct Platform {
    base: String,
    sent: Arc<StdMutex<Vec<(String, Value)>>>,
    delivered: Arc<Notify>,
}

async fn platform(replies: Vec<Value>) -> Platform {
    let sent = Arc::new(StdMutex::new(Vec::new()));
    let delivered = Arc::new(Notify::new());
    let replies = Arc::new(StdMutex::new(replies));
    let record = {
        let sent = sent.clone();
        let delivered = delivered.clone();
        move |path: String, headers: HeaderMap, body: Value| {
            assert_eq!(headers["authorization"], "QQBot offline-token");
            sent.lock().unwrap().push((path, body));
            delivered.notify_one();
            let mut replies = replies.lock().unwrap();
            if replies.len() > 1 {
                replies.remove(0)
            } else {
                replies[0].clone()
            }
        }
    };
    let users = record.clone();
    let groups = record;
    let app = Router::new()
        .route(
            "/token",
            post(|Json(body): Json<Value>| async move {
                assert_eq!(
                    body,
                    json!({"appId":"app-1","clientSecret":"offline-secret"})
                );
                Json(json!({"access_token":"offline-token","expires_in":"7200"}))
            }),
        )
        .route(
            "/gateway",
            get(|| async { Json(json!({"url":"wss://api.sgroup.qq.com/websocket"})) }),
        )
        .route(
            "/v2/users/{id}/messages",
            post(move |headers: HeaderMap, Json(body): Json<Value>| {
                let record = users.clone();
                async move { Json(record("users".to_owned(), headers, body)) }
            }),
        )
        .route(
            "/v2/groups/{id}/messages",
            post(move |headers: HeaderMap, Json(body): Json<Value>| {
                let record = groups.clone();
                async move { Json(record("groups".to_owned(), headers, body)) }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Platform {
        base,
        sent,
        delivered,
    }
}

fn plain_client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}

async fn post_once(
    replies: Vec<Value>,
    passive: Option<(&str, usize)>,
) -> (Result<(), ErrorCode>, Vec<Value>) {
    let platform = platform(replies).await;
    let fixture = Fixture::new();
    let bots = fixture.bots();
    let channel = channel(false);
    let client = plain_client();
    let secret = SecretString::from("offline-secret".to_owned());
    let token_url = format!("{}/token", platform.base);
    let connection = Connection {
        bots: &bots,
        channel: &channel,
        client: &client,
        app_id: "app-1",
        secret: &secret,
        hosts: Hosts {
            token: &token_url,
            api: &platform.base,
        },
        token: Mutex::new(None),
    };
    let result = connection
        .post_text("/v2/users/abc/messages", "hello", passive)
        .await;
    bots.shutdown().await.unwrap();
    let bodies = platform
        .sent
        .lock()
        .unwrap()
        .iter()
        .map(|(_, body)| body.clone())
        .collect();
    (result, bodies)
}

#[tokio::test]
async fn replies_inside_the_window_reference_the_inbound_message() {
    let (result, bodies) = post_once(vec![json!({"id":"sent-1"})], Some(("msg-1", 2))).await;
    result.unwrap();
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0]["msg_id"], "msg-1");
    assert_eq!(bodies[0]["msg_seq"], 2);
}

#[tokio::test]
async fn a_closed_reply_window_falls_back_to_a_proactive_message() {
    for code in REPLY_WINDOW_CLOSED {
        let (result, bodies) = post_once(
            vec![
                json!({"code":code,"message":"closed"}),
                json!({"id":"sent-1"}),
            ],
            Some(("msg-1", 1)),
        )
        .await;
        result.unwrap();
        assert_eq!(bodies.len(), 2);
        assert!(bodies[1].get("msg_id").is_none());
        assert!(bodies[1].get("msg_seq").is_none());
    }
}

#[tokio::test]
async fn other_refusals_fail_without_sending_proactively() {
    let (result, bodies) = post_once(
        vec![json!({"err_code":11_244,"message":"nope"})],
        Some(("msg-1", 1)),
    )
    .await;
    assert_eq!(result, Err(ErrorCode::BotIo));
    assert_eq!(bodies.len(), 1);
}

#[tokio::test]
async fn an_http_failure_without_a_qq_code_is_never_a_delivered_send() {
    let app = Router::new()
        .route(
            "/token",
            post(|| async { Json(json!({"access_token":"offline-token","expires_in":7200})) }),
        )
        .route(
            "/v2/users/abc/messages",
            post(|| async {
                (
                    axum::http::StatusCode::BAD_GATEWAY,
                    Json(json!({"detail":"upstream"})),
                )
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let fixture = Fixture::new();
    let bots = fixture.bots();
    let channel = channel(false);
    let client = plain_client();
    let secret = SecretString::from("offline-secret".to_owned());
    let token_url = format!("{base}/token");
    let connection = Connection {
        bots: &bots,
        channel: &channel,
        client: &client,
        app_id: "app-1",
        secret: &secret,
        hosts: Hosts {
            token: &token_url,
            api: &base,
        },
        token: Mutex::new(None),
    };
    let sent = connection
        .post_text("/v2/users/abc/messages", "hello", None)
        .await;
    assert_eq!(sent, Err(ErrorCode::BotIo));
    bots.shutdown().await.unwrap();
}

#[tokio::test]
async fn heartbeats_continue_while_a_message_waits_for_admission() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
        let frame = socket.next().await.unwrap().unwrap();
        let first = serde_json::from_str::<Value>(frame.to_text().unwrap()).unwrap();
        while socket.next().await.is_some() {}
        first
    });
    let (mut socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}"))
        .await
        .unwrap();
    let mut heartbeat = tokio::time::interval(Duration::from_millis(5));
    let finished = keep_alive(
        &mut socket,
        &mut heartbeat,
        Some(4),
        tokio::time::sleep(Duration::from_millis(50)),
    )
    .await;
    assert_eq!(finished, Ok(()));
    drop(socket);
    let received = tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(received, json!({"op":1,"d":4}));
}

async fn registered_channel(fixture: &Fixture, bots: &Bots, senders: Value) -> Channel {
    bots.execute("bot.setup.request", setup(WORKSPACE))
        .await
        .unwrap();
    let created = bots
        .execute(
            "bot.channel.create.request",
            json!({
                "workspaceId":WORKSPACE,"channelId":ChannelId::generate(),"enabled":true,
                "config":{"kind":"qq","appId":"app-1",
                    "secretRef":"env:AIT_SERVER_CREDENTIAL_TEST",
                    "allowedSenders":senders,"allowGroups":true},
            }),
        )
        .await
        .unwrap();
    fixture.channel(&created["channel"])
}

/// A gateway that says Hello, checks Identify, reports READY, then plays `events`.
async fn gateway(events: Vec<Value>) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
        let text = |frame: Value| WsMessage::Text(frame.to_string().into());
        socket
            .send(text(json!({"op":10,"d":{"heartbeat_interval":41_250}})))
            .await
            .unwrap();
        let identify = socket.next().await.unwrap().unwrap();
        let identify: Value = serde_json::from_str(identify.to_text().unwrap()).unwrap();
        assert_eq!(identify["op"], 2);
        assert_eq!(identify["d"]["token"], "QQBot offline-token");
        assert_eq!(identify["d"]["intents"], 1 << 25);
        socket
            .send(text(
                json!({"op":0,"s":1,"t":"READY","d":{"session_id":"s"}}),
            ))
            .await
            .unwrap();
        for event in events {
            socket.send(text(event)).await.unwrap();
        }
        while socket.next().await.is_some() {}
    });
    address
}

async fn drive_until<V>(
    bots: &Bots,
    channel: &Channel,
    platform: &Platform,
    address: std::net::SocketAddr,
    verify: impl FnOnce(CancellationToken) -> V,
) where
    V: Future<Output = ()>,
{
    let client = plain_client();
    let secret = SecretString::from("offline-secret".to_owned());
    let token_url = format!("{}/token", platform.base);
    let hosts = Hosts {
        token: &token_url,
        api: &platform.base,
    };
    let cancel = CancellationToken::new();
    let driver = run_at(
        bots,
        channel,
        (&client, &secret, hosts),
        &cancel,
        move |endpoint| async move {
            assert_eq!(endpoint, "wss://api.sgroup.qq.com/websocket");
            tokio_tungstenite::connect_async(format!("ws://{address}"))
                .await
                .map(|(socket, _)| socket)
                .map_err(|_| ErrorCode::BotIo)
        },
    );
    let verify = verify(cancel.clone());
    let (result, ()) = Box::pin(tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(driver, verify)
    }))
    .await
    .unwrap();
    result.unwrap();
}

#[tokio::test]
async fn a_private_message_is_queued_and_answered_as_a_reply() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    let channel = registered_channel(&fixture, &bots, json!(["user-open"])).await;
    let platform = platform(vec![json!({"id":"sent-1"})]).await;
    let event = json!({"op":0,"s":2,"t":PRIVATE_MESSAGE,"d":private_event()});
    let address = gateway(vec![event]).await;
    let (fixture_view, platform_view) = (&fixture, &platform);
    drive_until(&bots, &channel, &platform, address, |cancel| async move {
        platform_view.delivered.notified().await;
        loop {
            let conversations = fixture_view.store.conversations().unwrap();
            let turns = fixture_view.store.turns(conversations[0].id).unwrap();
            if turns[0].delivery == domain::bot::DeliveryStatus::Delivered {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        cancel.cancel();
    })
    .await;
    let sent = platform.sent.lock().unwrap().clone();
    assert_eq!(sent[0].0, "users");
    assert_eq!(sent[0].1["content"], "reply:hello");
    assert_eq!(sent[0].1["msg_id"], "msg-1");
    assert_eq!(sent[0].1["msg_seq"], 1);
    bots.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_unknown_group_member_is_ignored_until_they_present_the_pairing_code() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    let channel = registered_channel(&fixture, &bots, json!(["user-open"])).await;
    let paired = bots
        .execute(
            "bot.channel.qq.pair.request",
            json!({"workspaceId":WORKSPACE,"channelId":channel.id}),
        )
        .await
        .unwrap();
    let code = paired["code"].as_str().unwrap().to_owned();
    let platform = platform(vec![json!({"id":"sent-1"})]).await;
    let mut wrong = group_event();
    wrong["content"] = json!("<@!bot> not-the-code");
    let mut right = group_event();
    right["id"] = json!("msg-3");
    right["content"] = json!(format!("<@!bot> {code}"));
    let address = gateway(vec![
        json!({"op":0,"s":2,"t":GROUP_MESSAGE,"d":wrong}),
        json!({"op":0,"s":3,"t":GROUP_MESSAGE,"d":right}),
    ])
    .await;
    let (fixture_view, platform_view) = (&fixture, &platform);
    let channel_id = channel.id;
    drive_until(&bots, &channel, &platform, address, |cancel| async move {
        platform_view.delivered.notified().await;
        loop {
            let catalog = fixture_view.store.catalog().unwrap();
            let saved = catalog
                .channels
                .iter()
                .find(|c| c.id == channel_id)
                .unwrap();
            if matches!(&saved.config, ChannelConfig::Qq { allowed_senders, .. }
                if allowed_senders.len() == 2)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        cancel.cancel();
    })
    .await;
    let sent = platform.sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "groups");
    assert_eq!(sent[0].1["content"], PAIRED_NOTICE);
    assert_eq!(sent[0].1["msg_id"], "msg-3");
    assert!(fixture.store.conversations().unwrap().is_empty());
    let catalog = fixture.store.catalog().unwrap();
    let saved = catalog
        .channels
        .iter()
        .find(|c| c.id == channel.id)
        .unwrap();
    assert!(
        matches!(&saved.config, ChannelConfig::Qq { allowed_senders, .. }
        if allowed_senders == &["user-open".to_owned(), "member-open".to_owned()])
    );
    assert!(!bots.claim_qq_pairing(channel.id, &code));
    bots.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_channel_with_pasted_credentials_may_start_without_senders() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    let channel = registered_channel(&fixture, &bots, json!([])).await;
    assert!(
        matches!(&channel.config, ChannelConfig::Qq { allowed_senders, .. }
        if allowed_senders.is_empty())
    );
    bots.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_channel_of_another_kind_still_needs_a_sender() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    bots.execute("bot.setup.request", setup(WORKSPACE))
        .await
        .unwrap();
    let refused = bots
        .execute(
            "bot.channel.create.request",
            json!({
                "workspaceId":WORKSPACE,"channelId":ChannelId::generate(),"enabled":false,
                "config":{"kind":"weixin","accountId":"bot","baseUrl":API_FOR_VALIDATION,
                    "tokenRef":"env:AIT_SERVER_CREDENTIAL_TEST","allowedSenders":[]},
            }),
        )
        .await;
    assert_eq!(refused, Err(ErrorCode::InvalidMessage));
    bots.shutdown().await.unwrap();
}

const API_FOR_VALIDATION: &str = "https://ilinkai.weixin.qq.com";
