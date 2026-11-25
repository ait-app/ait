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

fn full_group_event(content: &str) -> Value {
    let mut event = group_event();
    event["content"] = json!(content);
    event
}

fn legacy_group_event(content: &str) -> Value {
    let mut event = full_group_event(content);
    event["author"] = json!({"id":"member-open"});
    event
}

#[test]
fn private_messages_route_to_the_sender() {
    let routed = inbound(&channel(false), PRIVATE_MESSAGE, &private_event())
        .unwrap()
        .unwrap();
    assert_eq!(routed.peer, "c2c:user-open");
    assert_eq!(routed.sender, "user-open");
    assert_eq!(routed.text, "hello");
    assert_eq!(routed.message_id, "msg-1");
    assert_eq!(routed.reply_to.as_deref(), Some("msg-1"));
}

#[test]
fn long_message_ids_use_distinct_stable_keys_in_private_and_group_chats() {
    let channel = channel(true);
    let first_id = format!("ROBOT1.0 {}first", "a".repeat(128));
    let second_id = format!("ROBOT1.0 {}second", "a".repeat(128));
    for event in [PRIVATE_MESSAGE, GROUP_AT_MESSAGE, GROUP_ALL_MESSAGE] {
        let mut data = if event == PRIVATE_MESSAGE {
            private_event()
        } else {
            group_event()
        };
        data["id"] = json!(first_id);
        let first = inbound(&channel, event, &data).unwrap().unwrap();
        let repeated = inbound(&channel, event, &data).unwrap().unwrap();
        data["id"] = json!(second_id);
        let second = inbound(&channel, event, &data).unwrap().unwrap();

        assert!(model::valid_id(&first.message_id));
        assert!(model::valid_id(&second.message_id));
        assert_eq!(first.message_id, repeated.message_id);
        assert_ne!(first.message_id, second.message_id);
        assert_eq!(first.reply_to.as_deref(), Some(first_id.as_str()));
        assert_eq!(second.reply_to.as_deref(), Some(second_id.as_str()));
    }
}

#[test]
fn group_mentions_need_the_channel_to_allow_groups() {
    let routed = inbound(&channel(true), GROUP_AT_MESSAGE, &group_event())
        .unwrap()
        .unwrap();
    assert_eq!(routed.peer, "group:group-1");
    assert_eq!(routed.sender, "member-open");
    assert_eq!(routed.text, "hi there");
    assert!(
        inbound(&channel(false), GROUP_AT_MESSAGE, &group_event())
            .unwrap()
            .is_none()
    );
}

#[test]
fn full_group_events_route_even_without_a_bot_mention() {
    let groups_allowed = channel(true);
    let routed = inbound(
        &groups_allowed,
        GROUP_ALL_MESSAGE,
        &full_group_event("hello everyone"),
    )
    .unwrap()
    .unwrap();
    assert_eq!(routed.peer, "group:group-1");
    assert_eq!(routed.sender, "member-open");
    assert_eq!(routed.text, "hello everyone");
    assert!(
        inbound(
            &channel(false),
            GROUP_ALL_MESSAGE,
            &full_group_event("hello everyone"),
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn group_events_fall_back_to_the_legacy_author_id() {
    let routed = inbound(
        &channel(true),
        GROUP_ALL_MESSAGE,
        &legacy_group_event("hello everyone"),
    )
    .unwrap()
    .unwrap();
    assert_eq!(routed.peer, "group:group-1");
    assert_eq!(routed.sender, "member-open");
    assert_eq!(routed.text, "hello everyone");
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
fn passive_replies_allow_five_chunks_per_inbound_message() {
    assert_eq!(passive_reply(Some("msg-1"), 4), Some(("msg-1", 5)));
    assert_eq!(passive_reply(Some("msg-1"), 5), None);
}

#[test]
fn refusal_accepts_numeric_and_string_error_codes() {
    assert_eq!(refusal(&json!({"code": 40_034_005})), Some(40_034_005));
    assert_eq!(refusal(&json!({"err_code": "40034026"})), Some(40_034_026));
    assert_eq!(refusal(&json!({"code": 0})), None);
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
        image_directory: None,
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
async fn a_closed_reply_window_is_not_retried_as_a_proactive_message() {
    for code in REPLY_WINDOW_CLOSED {
        let (result, bodies) = post_once(
            vec![json!({"code":code,"message":"closed"})],
            Some(("msg-1", 1)),
        )
        .await;
        assert_eq!(result, Err(ErrorCode::BotIo));
        assert_eq!(bodies.len(), 1);
        assert_eq!(bodies[0]["msg_id"], "msg-1");
    }
}

#[tokio::test]
async fn a_reply_without_an_inbound_anchor_is_rejected_without_sending() {
    let (result, bodies) = post_once(vec![json!({"id":"sent-1"})], None).await;
    assert_eq!(result, Err(ErrorCode::BotIo));
    assert!(bodies.is_empty());
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
        image_directory: None,
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
    assert_private_reply("msg-1").await;
}

#[tokio::test]
async fn a_long_private_message_id_is_deduplicated_and_preserved_for_replies() {
    let message_id = format!("ROBOT1.0 {}", "a".repeat(137));
    assert_private_reply(&message_id).await;
}

async fn assert_private_reply(message_id: &str) {
    let mut data = private_event();
    data["id"] = json!(message_id);
    assert_message_reply(
        PRIVATE_MESSAGE,
        data,
        "user-open",
        "c2c:user-open",
        "reply:hello",
    )
    .await;
}

#[tokio::test]
async fn unpaired_group_members_mentions_are_answered_without_pairing() {
    assert_message_reply(
        GROUP_AT_MESSAGE,
        group_event(),
        "member-open",
        "group:group-1",
        "reply:hi there",
    )
    .await;
}

#[tokio::test]
async fn unpaired_group_members_mentions_in_full_group_events_are_answered() {
    let mut data = legacy_group_event("<@!bot> hello");
    data["mentions"] = json!([{"member_openid":"bot","is_you":true}]);
    assert_message_reply(
        GROUP_ALL_MESSAGE,
        data,
        "member-open",
        "group:group-1",
        "reply:hello",
    )
    .await;
}

#[tokio::test]
async fn trusted_senders_full_group_messages_still_work_without_mentions() {
    let mut data = full_group_event("hello everyone");
    data["author"]["member_openid"] = json!("user-open");
    assert_message_reply(
        GROUP_ALL_MESSAGE,
        data,
        "user-open",
        "group:group-1",
        "reply:hello everyone",
    )
    .await;
}

#[tokio::test]
async fn disabled_groups_are_ignored_for_all_senders() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    let channel = registered_channel(&fixture, &bots, json!(["user-open"])).await;
    let platform = platform(vec![json!({"id":"sent-1"})]).await;
    let client = plain_client();
    let secret = SecretString::from("offline-secret".to_owned());
    let token_url = format!("{}/token", platform.base);
    let hosts = Hosts {
        token: &token_url,
        api: &platform.base,
    };
    let mut mentioned = group_event();
    mentioned["mentions"] = json!([{"is_you":true}]);
    for (event, data, allow_groups) in [
        (GROUP_AT_MESSAGE, group_event(), false),
        (GROUP_ALL_MESSAGE, mentioned, false),
    ] {
        let mut channel = channel.clone();
        let ChannelConfig::Qq {
            allow_groups: enabled,
            ..
        } = &mut channel.config
        else {
            panic!("expected QQ channel");
        };
        *enabled = allow_groups;
        let connection = Connection {
            bots: &bots,
            channel: &channel,
            client: &client,
            app_id: "app-1",
            secret: &secret,
            hosts,
            token: Mutex::new(None),
            image_directory: None,
        };

        connection
            .dispatch(&json!({"t":event,"d":data}))
            .await
            .unwrap();

        assert!(fixture.store.conversations().unwrap().is_empty(), "{event}");
        assert!(platform.sent.lock().unwrap().is_empty(), "{event}");
    }
    bots.shutdown().await.unwrap();
}

async fn assert_message_reply(event: &str, data: Value, sender: &str, peer: &str, reply: &str) {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    let channel = registered_channel(&fixture, &bots, json!(["user-open"])).await;
    let platform = platform(vec![json!({"id":"sent-1"})]).await;
    let message_id = data["id"].as_str().unwrap().to_owned();
    let event = json!({"op":0,"s":2,"t":event,"d":data});
    let address = gateway(vec![event.clone(), event]).await;
    let (fixture_view, platform_view) = (&fixture, &platform);
    drive_until(&bots, &channel, &platform, address, |cancel| async move {
        platform_view.delivered.notified().await;
        loop {
            let conversations: Vec<_> = fixture_view
                .store
                .conversations()
                .unwrap()
                .into_iter()
                .filter(|c| c.channel_id == channel.id)
                .collect();
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
    assert_eq!(sent.len(), 1);
    let destination = if peer.starts_with("group:") {
        "groups"
    } else {
        "users"
    };
    assert_eq!(sent[0].0, destination);
    assert_eq!(sent[0].1["content"], reply);
    assert_eq!(sent[0].1["msg_id"], message_id);
    assert_eq!(sent[0].1["msg_seq"], 1);
    let conversations: Vec<_> = fixture
        .store
        .conversations()
        .unwrap()
        .into_iter()
        .filter(|c| c.channel_id == channel.id)
        .collect();
    assert_eq!(conversations.len(), 1);
    assert_eq!(conversations[0].sender, sender);
    assert_eq!(conversations[0].peer, peer);
    assert_eq!(fixture.store.turns(conversations[0].id).unwrap().len(), 1);
    let catalog = fixture.store.catalog().unwrap();
    let saved = catalog
        .channels
        .iter()
        .find(|c| c.id == channel.id)
        .unwrap();
    assert!(
        matches!(&saved.config, ChannelConfig::Qq { allowed_senders, .. }
        if allowed_senders == &["user-open".to_owned()])
    );
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
async fn all_private_senders_receive_replies_without_pairing() {
    let mut data = private_event();
    data["author"]["user_openid"] = json!("another-open");
    assert_message_reply(
        PRIVATE_MESSAGE,
        data,
        "another-open",
        "c2c:another-open",
        "reply:hello",
    )
    .await;
}

#[tokio::test]
async fn all_group_senders_receive_replies_without_mentions() {
    assert_message_reply(
        GROUP_ALL_MESSAGE,
        full_group_event("hello everyone"),
        "member-open",
        "group:group-1",
        "reply:hello everyone",
    )
    .await;
}
