use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::http::HeaderMap;
use axum::{Json, Router, routing::post};
use domain::{BotId, ChannelId};
use model::bot::Store;
use tokio::sync::Notify;

use super::*;
use crate::test_support::{Fixture, WORKSPACE, setup};

const SENDER: &str = "alice@im.wechat";
const ACCOUNT: &str = "bot-1@im.bot";

fn channel() -> Channel {
    Channel {
        id: ChannelId::generate(),
        bot_id: BotId::generate(),
        enabled: true,
        cursor: None,
        sync_token: None,
        deleted: false,
        config: ChannelConfig::Weixin {
            account_id: ACCOUNT.into(),
            base_url: API.into(),
            token_ref: "env:AIT_SERVER_CREDENTIAL_WEIXIN".into(),
            allowed_senders: vec![SENDER.into()],
        },
    }
}

fn message() -> Value {
    json!({
        "from_user_id":SENDER,
        "to_user_id":ACCOUNT,
        "message_id":7,
        "message_type":1,
        "context_token":"context-1",
        "item_list":[{"type":1,"text_item":{"text":"hello"}}],
    })
}

#[test]
fn private_messages_from_the_scanner_keep_their_conversation_token() {
    let routed = incoming(&channel(), &message(), ACCOUNT).unwrap().unwrap();
    assert_eq!(routed.peer, SENDER);
    assert_eq!(routed.sender, SENDER);
    assert_eq!(routed.message_id, "7");
    assert_eq!(routed.text, "hello");
    assert_eq!(routed.reply_to.as_deref(), Some("context-1"));
}

#[test]
fn other_senders_bots_groups_and_non_text_are_dropped() {
    let channel = channel();
    let dropped = |change: fn(&mut Value)| {
        let mut message = message();
        change(&mut message);
        incoming(&channel, &message, ACCOUNT).unwrap().is_none()
    };
    assert!(dropped(|message| message["from_user_id"] = json!("mallory")));
    assert!(dropped(|message| message["from_user_id"] = json!(ACCOUNT)));
    assert!(dropped(|message| message["message_type"] = json!(2)));
    assert!(dropped(|message| message["room_id"] = json!("room-1")));
    assert!(dropped(|message| message["chat_room_id"] = json!("room-1")));
    assert!(dropped(|message| message["group_id"] = json!("group-1")));
    assert!(dropped(|message| message["item_list"] = json!([{"type":2}])));
    assert!(dropped(
        |message| message["item_list"][0]["text_item"]["text"] = json!("  ")
    ));
    assert!(dropped(|message| {
        message["message_id"] = Value::Null;
    }));
}

#[test]
fn a_message_without_an_id_falls_back_to_its_sequence() {
    let mut message = message();
    message["message_id"] = Value::Null;
    message["seq"] = json!(9);
    let routed = incoming(&channel(), &message, ACCOUNT).unwrap().unwrap();
    assert_eq!(routed.message_id, format!("{SENDER}:9"));
}

#[test]
fn only_https_wechat_roots_may_receive_the_token() {
    assert!(trusted_base(API));
    assert!(trusted_base("https://sh.weixin.qq.com"));
    for base in [
        "http://ilinkai.weixin.qq.com",
        "https://weixin.qq.com.evil.example",
        "https://evilweixin.qq.com",
        "https://ilinkai.weixin.qq.com/path",
        "https://ilinkai.weixin.qq.com:8443",
        "https://user@ilinkai.weixin.qq.com",
        "not a url",
    ] {
        assert!(!trusted_base(base), "{base}");
    }
}

#[test]
fn ilink_errors_separate_expired_sessions_from_rate_limits() {
    assert_eq!(classify(&json!({})), Reply::Accepted);
    assert_eq!(classify(&json!({"ret":0,"errcode":0})), Reply::Accepted);
    assert_eq!(classify(&json!({"ret":-14})), Reply::Expired);
    assert_eq!(classify(&json!({"errcode":-14})), Reply::Expired);
    assert_eq!(
        classify(&json!({"ret":-2,"errmsg":"Prepare Failed"})),
        Reply::Expired
    );
    assert_eq!(classify(&json!({"ret":-2})), Reply::RateLimited);
    assert_eq!(classify(&json!({"ret":5})), Reply::Rejected);
}

async fn registered_channel(fixture: &Fixture, bots: &Bots) -> Channel {
    bots.execute("bot.setup.request", setup(WORKSPACE))
        .await
        .unwrap();
    let created = bots
        .execute(
            "bot.channel.create.request",
            json!({
                "workspaceId":WORKSPACE,"channelId":ChannelId::generate(),"enabled":true,
                "config":{"kind":"weixin","accountId":ACCOUNT,"baseUrl":API,
                    "tokenRef":"env:AIT_SERVER_CREDENTIAL_TEST","allowedSenders":[SENDER]},
            }),
        )
        .await
        .unwrap();
    fixture.channel(&created["channel"])
}

async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

fn plain_client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}

#[tokio::test]
async fn an_inbound_message_is_queued_answered_and_its_position_saved() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    let channel = registered_channel(&fixture, &bots).await;
    let polls = Arc::new(AtomicUsize::new(0));
    let sent = Arc::new(StdMutex::new(Vec::new()));
    let delivered = Arc::new(Notify::new());
    let app = Router::new()
        .route(
            "/ilink/bot/getupdates",
            post({
                let polls = polls.clone();
                move |headers: HeaderMap, Json(body): Json<Value>| {
                    let polls = polls.clone();
                    async move {
                        assert_eq!(headers["authorization"], "Bearer offline-token");
                        assert_eq!(headers["authorizationtype"], "ilink_bot_token");
                        assert_eq!(headers["ilink-app-id"], "bot");
                        assert_eq!(body["base_info"]["channel_version"], "2.2.0");
                        if polls.fetch_add(1, Ordering::SeqCst) > 0 {
                            assert_eq!(body["get_updates_buf"], "position-1");
                            std::future::pending::<()>().await;
                        }
                        Json(json!({"ret":0,"get_updates_buf":"position-1","msgs":[message()]}))
                    }
                }
            }),
        )
        .route(
            "/ilink/bot/sendmessage",
            post({
                let sent = sent.clone();
                let delivered = delivered.clone();
                move |Json(body): Json<Value>| {
                    let sent = sent.clone();
                    let delivered = delivered.clone();
                    async move {
                        sent.lock().unwrap().push(body);
                        delivered.notify_one();
                        Json(json!({"ret":0}))
                    }
                }
            }),
        );
    let base = serve(app).await;
    let client = plain_client();
    let token = SecretString::from("offline-token".to_owned());
    let cancel = CancellationToken::new();
    let driver = run_at(&bots, &channel, (&client, &token, &base), &cancel);
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
    let body = sent.lock().unwrap()[0].clone();
    assert_eq!(body["msg"]["to_user_id"], SENDER);
    assert_eq!(body["msg"]["context_token"], "context-1");
    assert_eq!(
        body["msg"]["item_list"][0]["text_item"]["text"],
        "reply:hello"
    );
    let saved = fixture.store.catalog().unwrap();
    let saved = saved
        .channels
        .iter()
        .find(|entry| entry.id == channel.id)
        .unwrap();
    assert_eq!(saved.sync_token.as_deref(), Some("position-1"));
    bots.shutdown().await.unwrap();
}

async fn send_with_responses(responses: Vec<Value>) -> (Result<(), ErrorCode>, Vec<Value>) {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    let channel = channel();
    let seen = Arc::new(StdMutex::new(Vec::new()));
    let replies = Arc::new(StdMutex::new(responses));
    let app = Router::new().route(
        "/ilink/bot/sendmessage",
        post({
            let seen = seen.clone();
            move |Json(body): Json<Value>| {
                let seen = seen.clone();
                let replies = replies.clone();
                async move {
                    seen.lock().unwrap().push(body);
                    Json(replies.lock().unwrap().remove(0))
                }
            }
        }),
    );
    let base = serve(app).await;
    let client = plain_client();
    let token = SecretString::from("offline-token".to_owned());
    let connection = Connection {
        bots: &bots,
        channel: &channel,
        client: &client,
        token: &token,
        base: &base,
        account_id: ACCOUNT,
    };
    let result = connection
        .post_text(SENDER, Some("old-context"), "hello", "client-1")
        .await;
    bots.shutdown().await.unwrap();
    let seen = seen.lock().unwrap().clone();
    (result, seen)
}

#[tokio::test]
async fn a_retired_conversation_token_is_dropped_for_one_retry() {
    let (result, seen) = send_with_responses(vec![json!({"ret":-14}), json!({"ret":0})]).await;
    result.unwrap();
    assert_eq!(seen[0]["msg"]["context_token"], "old-context");
    assert!(seen[1]["msg"].get("context_token").is_none());
    assert_eq!(seen[1]["msg"]["client_id"], "client-1");
}

#[tokio::test]
async fn a_session_that_stays_expired_fails_the_send() {
    let (result, seen) = send_with_responses(vec![json!({"ret":-14}), json!({"ret":-14})]).await;
    assert_eq!(result, Err(ErrorCode::BotIo));
    assert_eq!(seen.len(), 2);
}

#[tokio::test]
async fn an_http_failure_is_never_a_delivered_send() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    let channel = channel();
    let app = Router::new().route(
        "/ilink/bot/sendmessage",
        post(|| async {
            (
                axum::http::StatusCode::BAD_GATEWAY,
                Json(json!({"detail":"upstream"})),
            )
        }),
    );
    let base = serve(app).await;
    let client = plain_client();
    let token = SecretString::from("offline-token".to_owned());
    let connection = Connection {
        bots: &bots,
        channel: &channel,
        client: &client,
        token: &token,
        base: &base,
        account_id: ACCOUNT,
    };
    let sent = connection
        .post_text(SENDER, None, "hello", "client-1")
        .await;
    assert_eq!(sent, Err(ErrorCode::BotIo));
    bots.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_failed_poll_retries_from_the_position_already_reached() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    let channel = registered_channel(&fixture, &bots).await;
    let buffers = Arc::new(StdMutex::new(Vec::new()));
    let finished = Arc::new(Notify::new());
    let app = Router::new().route(
        "/ilink/bot/getupdates",
        post({
            let buffers = buffers.clone();
            let finished = finished.clone();
            move |Json(body): Json<Value>| {
                let buffers = buffers.clone();
                let finished = finished.clone();
                async move {
                    let call = {
                        let mut buffers = buffers.lock().unwrap();
                        buffers.push(body["get_updates_buf"].clone());
                        buffers.len()
                    };
                    match call {
                        1 => (
                            axum::http::StatusCode::OK,
                            Json(json!({"ret":0,"get_updates_buf":"position-1"})),
                        ),
                        2 => (axum::http::StatusCode::BAD_GATEWAY, Json(json!({}))),
                        _ => {
                            finished.notify_one();
                            std::future::pending().await
                        }
                    }
                }
            }
        }),
    );
    let base = serve(app).await;
    let client = plain_client();
    let token = SecretString::from("offline-token".to_owned());
    let cancel = CancellationToken::new();
    let driver = run_at(&bots, &channel, (&client, &token, &base), &cancel);
    let verify = async {
        finished.notified().await;
        cancel.cancel();
    };
    let (result, ()) = Box::pin(tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(driver, verify)
    }))
    .await
    .unwrap();
    result.unwrap();
    assert_eq!(buffers.lock().unwrap()[2], "position-1");
    bots.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_ambiguous_poll_error_does_not_claim_the_session_ended() {
    assert!(!session_expired(
        &json!({"ret":-2,"errmsg":"unknown error"})
    ));
    assert!(session_expired(&json!({"errcode":-14})));
}

#[tokio::test]
async fn an_expired_session_is_reported_on_the_channel() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    let channel = registered_channel(&fixture, &bots).await;
    let app = Router::new().route(
        "/ilink/bot/getupdates",
        post(|| async { Json(json!({"ret":-14})) }),
    );
    let base = serve(app).await;
    let client = plain_client();
    let token = SecretString::from("offline-token".to_owned());
    let cancel = CancellationToken::new();
    let driver = run_at(&bots, &channel, (&client, &token, &base), &cancel);
    let verify = async {
        loop {
            let status = bots
                .execute(
                    "bot.channel.status.request",
                    json!({"workspaceId":WORKSPACE,"channelId":channel.id}),
                )
                .await
                .unwrap();
            if status["connection"]["state"] == "expired" {
                assert_eq!(status["connection"]["error"], EXPIRED_NOTICE);
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
}
