use std::sync::{Arc, Mutex as StdMutex};

use axum::http::{HeaderMap, StatusCode};
use axum::{Json, Router, routing::post};
use domain::{BotId, ChannelId};
use model::bot::Store;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::handshake::server::{
    ErrorResponse, Request as ServerRequest, Response as ServerResponse,
};

use super::*;
use crate::service::Admission;
use crate::test_support::{Fixture, WORKSPACE, setup};

const PROJECT: &str = "pho_prj_0123456789abcdefghjkmnpqrs";
const OWNER: &str = "+15551234567";
const SECOND_OWNER: &str = "owner@example.com";
const SUBSCRIBED: &str = r#"430[{"ok":true}]"#;

fn config() -> ChannelConfig {
    ChannelConfig::PhotonImessage {
        project_id: PROJECT.into(),
        key_ref: "env:AIT_SERVER_CREDENTIAL_TEST".into(),
        allowed_senders: vec![OWNER.into(), SECOND_OWNER.into()],
    }
}

fn channel() -> Channel {
    Channel {
        id: ChannelId::generate(),
        bot_id: BotId::generate(),
        enabled: true,
        cursor: None,
        sync_token: None,
        deleted: false,
        config: config(),
    }
}

fn event(id: &str, handle: &str, text: &str) -> Value {
    json!({"id":"evt","type":"message.received","data":{"object":{
        "id":id,
        "platform":"imessage",
        "status":"received",
        "content":{"type":"text","format":"plain","text":text},
        "sentBy":{"id":"pho_usr_sender","kind":"human","handle":handle},
        "receivedBy":{"id":"pho_usr_agent","kind":"agent","handle":"+15550000000"},
        "space":{"id":"pho_spc_one","kind":"direct"},
    }}})
}

fn delivery(sequence: i64, id: &str, handle: &str) -> String {
    let frame = json!(["delivery", event(id, handle, "hello"), {"seq":sequence}]);
    format!("42{frame}")
}

#[test]
fn a_direct_text_routes_to_its_space_and_agent() {
    let routed = incoming(&channel(), &event("pho_msg_1", OWNER, " hi ")).unwrap();
    assert_eq!(routed.peer, "pho_spc_one/pho_usr_agent");
    assert_eq!(routed.sender, OWNER);
    assert_eq!(routed.message_id, "pho_msg_1");
    assert_eq!(routed.text, " hi ");
    assert_eq!(routed.thread, None);
    assert_eq!(routed.reply_to, None);
}

#[test]
fn email_senders_match_regardless_of_case() {
    let routed = incoming(&channel(), &event("m", "OWNER@example.com", "hi")).unwrap();
    assert_eq!(routed.sender, "OWNER@example.com");
}

#[test]
fn events_the_channel_does_not_accept_are_ignored() {
    let channel = channel();
    let cases: [(&str, Value); 8] = [
        ("/type", json!("message.updated")),
        ("/data/object/platform", json!("sms")),
        ("/data/object/status", json!("accepted")),
        ("/data/object/sentBy/kind", json!("agent")),
        ("/data/object/sentBy/handle", json!("+15559999999")),
        ("/data/object/space/kind", json!("group")),
        ("/data/object/content/type", json!("reaction")),
        ("/data/object/receivedBy", Value::Null),
    ];
    for (path, value) in cases {
        let mut changed = event("m", OWNER, "hi");
        *changed.pointer_mut(path).unwrap() = value;
        assert!(incoming(&channel, &changed).is_none(), "{path}");
    }
}

#[test]
fn blank_oversized_and_pairing_code_texts_are_ignored() {
    let channel = channel();
    let code = format!("ait-{}", "0".repeat(32));
    let oversized = "x".repeat(MAX_TEXT_BYTES + 1);
    for text in ["  ", oversized.as_str(), code.as_str()] {
        assert!(incoming(&channel, &event("m", OWNER, text)).is_none());
    }
}

#[test]
fn project_ids_must_match_photons_shape() {
    assert!(valid_project_id(PROJECT));
    for project in [
        "",
        "pho_prj_",
        "pho_prj_../../../../../../../../..",
        "pho_prj_8123456789abcdefghjkmnpqrs",
        "pho_prj_0123456789abcdefghjkmnpqrl",
        "pho_prj_0123456789ABCDEFGHJKMNPQRS",
        "pho_sk_0123456789abcdefghjkmnpqrst",
    ] {
        assert!(!valid_project_id(project), "{project}");
    }
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
                "config":{"kind":"photon_imessage","projectId":PROJECT,
                    "keyRef":"env:AIT_SERVER_CREDENTIAL_TEST",
                    "allowedSenders":[OWNER,SECOND_OWNER]},
            }),
        )
        .await
        .unwrap();
    fixture.channel(&created["channel"])
}

#[tokio::test]
async fn a_malformed_project_id_cannot_be_saved() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    bots.execute("bot.setup.request", setup(WORKSPACE))
        .await
        .unwrap();
    let created = bots
        .execute(
            "bot.channel.create.request",
            json!({
                "workspaceId":WORKSPACE,"channelId":ChannelId::generate(),
                "config":{"kind":"photon_imessage","projectId":"pho_prj_/../x",
                    "keyRef":"env:AIT_SERVER_CREDENTIAL_TEST","allowedSenders":[OWNER]},
            }),
        )
        .await;
    assert_eq!(created, Err(ErrorCode::InvalidMessage));
}

#[derive(Debug, Default)]
struct Seen {
    uri: String,
    authorization: Option<String>,
    frames: Vec<String>,
}

async fn expect_frame(
    socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
) -> String {
    let frame = socket.next().await.unwrap().unwrap();
    frame.to_text().unwrap().to_owned()
}

/// Play Photon's side of one connection: handshake, a ping, the given frames, then a disconnect.
#[allow(clippy::result_large_err)]
async fn serve(answer: &'static str, frames: Vec<String>) -> (String, JoinHandle<Seen>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let headers = Arc::new(StdMutex::new(Seen::default()));
        let recorded = headers.clone();
        let callback = move |request: &ServerRequest, response: ServerResponse| {
            let mut seen = recorded.lock().unwrap();
            seen.uri = request.uri().to_string();
            seen.authorization = request
                .headers()
                .get("authorization")
                .map(|value| value.to_str().unwrap().to_owned());
            Ok::<_, ErrorResponse>(response)
        };
        let mut socket = tokio_tungstenite::accept_hdr_async(tcp, callback)
            .await
            .unwrap();
        let mut received = Vec::new();
        let open = r#"0{"sid":"s","pingInterval":25000,"pingTimeout":20000}"#;
        socket.send(WsMessage::Text(open.into())).await.unwrap();
        received.push(expect_frame(&mut socket).await);
        socket
            .send(WsMessage::Text(r#"40{"sid":"n"}"#.into()))
            .await
            .unwrap();
        received.push(expect_frame(&mut socket).await);
        socket.send(WsMessage::Text(answer.into())).await.unwrap();
        socket.send(WsMessage::Text("2".into())).await.unwrap();
        received.push(expect_frame(&mut socket).await);
        for frame in frames {
            socket.send(WsMessage::Text(frame.into())).await.unwrap();
        }
        // A second ping proves every frame above was handled before the disconnect.
        socket.send(WsMessage::Text("2".into())).await.unwrap();
        received.push(expect_frame(&mut socket).await);
        socket.send(WsMessage::Text("41".into())).await.unwrap();
        let mut seen = std::mem::take(&mut *headers.lock().unwrap());
        seen.frames = received;
        seen
    });
    (base, server)
}

async fn run_once(bots: &Bots, channel: &Channel, base: &str) -> Result<(), ErrorCode> {
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let key = SecretString::from("pho_sk_offline".to_owned());
    let cancel = CancellationToken::new();
    tokio::time::timeout(
        Duration::from_secs(10),
        run_at(bots, channel, (&client, &key, base), &cancel),
    )
    .await
    .unwrap()
}

async fn finished(server: JoinHandle<Seen>) -> Seen {
    tokio::time::timeout(Duration::from_secs(10), server)
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn the_stream_authenticates_subscribes_and_answers_pings() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    let channel = registered_channel(&fixture, &bots).await;
    let (base, server) = serve(SUBSCRIBED, Vec::new()).await;

    let result = run_once(&bots, &channel, &base).await;

    assert_eq!(result, Err(ErrorCode::BotIo));
    let seen = finished(server).await;
    assert_eq!(
        seen.uri,
        format!("/v1/projects/{PROJECT}/events/ws?EIO=4&transport=websocket")
    );
    assert_eq!(seen.authorization.as_deref(), Some("Bearer pho_sk_offline"));
    assert_eq!(seen.frames[0], "40");
    assert_eq!(
        seen.frames[1],
        r#"420["subscribe",{"apiVersion":"2026-07-01","startSeq":0}]"#
    );
    assert_eq!(seen.frames[2], "3");
    assert_eq!(seen.frames[3], "3");
}

#[tokio::test]
async fn deliveries_save_their_position_and_a_reconnect_resumes_from_it() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    let channel = registered_channel(&fixture, &bots).await;
    let (base, server) = serve(
        SUBSCRIBED,
        vec![
            delivery(3, "pho_msg_1", OWNER),
            // A replayed position must not reach the Bot, even from another allowed sender.
            delivery(3, "pho_msg_2", SECOND_OWNER),
            // Refused input still advances the position so it is never read again.
            delivery(9, "pho_msg_3", "+15559999999"),
        ],
    )
    .await;

    assert_eq!(
        run_once(&bots, &channel, &base).await,
        Err(ErrorCode::BotIo)
    );
    finished(server).await;

    assert_eq!(bots.channel_cursor(channel.id).await, Ok(Some(9)));
    let conversations: Vec<_> = fixture
        .store
        .conversations()
        .unwrap()
        .into_iter()
        .filter(|conversation| conversation.channel_id == channel.id)
        .collect();
    assert_eq!(conversations.len(), 1);
    assert_eq!(conversations[0].peer, "pho_spc_one/pho_usr_agent");
    assert_eq!(conversations[0].sender, OWNER);

    // `channel` is still the stale snapshot, as it is across real reconnects.
    let (base, server) = serve(SUBSCRIBED, Vec::new()).await;
    assert_eq!(
        run_once(&bots, &channel, &base).await,
        Err(ErrorCode::BotIo)
    );
    assert_eq!(
        finished(server).await.frames[1],
        r#"420["subscribe",{"apiVersion":"2026-07-01","startSeq":9}]"#
    );
}

#[tokio::test]
async fn a_refused_subscription_never_reports_connected() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    let channel = registered_channel(&fixture, &bots).await;
    let client = reqwest::Client::new();
    let key = SecretString::from("pho_sk_offline".to_owned());
    let connection = Connection {
        bots: &bots,
        channel: &channel,
        client: &client,
        key: &key,
        api: "http://unused",
        project_id: PROJECT,
        backlog_wait: Duration::from_millis(1),
    };
    let refused = json!({"ok":false,"error":{"code":"capacity","fatal":true}});

    assert_eq!(connection.subscribed(&refused).await, Err(ErrorCode::BotIo));
    assert_eq!(connection.subscribed(&json!({"ok":true})).await, Ok(()));
}

#[tokio::test]
async fn only_fatal_delivery_errors_end_the_connection() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    let channel = registered_channel(&fixture, &bots).await;
    let client = reqwest::Client::new();
    let key = SecretString::from("pho_sk_offline".to_owned());
    let connection = Connection {
        bots: &bots,
        channel: &channel,
        client: &client,
        key: &key,
        api: "http://unused",
        project_id: PROJECT,
        backlog_wait: Duration::from_millis(1),
    };
    let mut handled = None;
    let report = |fatal: bool| {
        vec![
            json!("delivery_error"),
            json!({"code":"frame_invalid","fatal":fatal}),
        ]
    };

    assert_eq!(connection.event(&report(false), &mut handled).await, Ok(()));
    assert_eq!(
        connection.event(&report(true), &mut handled).await,
        Err(ErrorCode::BotIo)
    );
    assert_eq!(handled, None);
}

type Sent = Arc<StdMutex<Vec<(Option<String>, Option<String>, Value)>>>;

async fn message_api(status: StatusCode) -> (String, Sent) {
    let sent = Sent::default();
    let record = sent.clone();
    let path = format!("/v1/projects/{PROJECT}/messages");
    let app = Router::new().route(
        &path,
        post(move |headers: HeaderMap, Json(body): Json<Value>| {
            let record = record.clone();
            async move {
                let header = |name: &str| {
                    headers
                        .get(name)
                        .map(|value| value.to_str().unwrap().to_owned())
                };
                record.lock().unwrap().push((
                    header("authorization"),
                    header("idempotency-key"),
                    body,
                ));
                status
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, sent)
}

#[tokio::test]
async fn sends_name_the_space_and_agent_from_the_peer() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    let channel = channel();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let key = SecretString::from("pho_sk_offline".to_owned());
    let (api, sent) = message_api(StatusCode::ACCEPTED).await;
    let connection = Connection {
        bots: &bots,
        channel: &channel,
        client: &client,
        key: &key,
        api: &api,
        project_id: PROJECT,
        backlog_wait: Duration::from_millis(1),
    };

    let result = connection
        .send("pho_spc_one/pho_usr_agent", "reply", "turn:0")
        .await;

    assert_eq!(result, Ok(()));
    let sent = sent.lock().unwrap();
    assert_eq!(sent[0].0.as_deref(), Some("Bearer pho_sk_offline"));
    assert_eq!(sent[0].1.as_deref(), Some("turn:0"));
    assert_eq!(
        sent[0].2,
        json!({"spaceId":"pho_spc_one","senderId":"pho_usr_agent",
            "content":{"type":"text","format":"plain","text":"reply"}})
    );
}

#[tokio::test]
async fn refused_sends_and_malformed_peers_fail() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    let channel = channel();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let key = SecretString::from("pho_sk_offline".to_owned());
    let (api, sent) = message_api(StatusCode::BAD_GATEWAY).await;
    let connection = Connection {
        bots: &bots,
        channel: &channel,
        client: &client,
        key: &key,
        api: &api,
        project_id: PROJECT,
        backlog_wait: Duration::from_millis(1),
    };

    assert_eq!(
        connection.send("space/agent", "reply", "k").await,
        Err(ErrorCode::BotIo)
    );
    assert_eq!(
        connection.send("no-separator", "reply", "k").await,
        Err(ErrorCode::BotIo)
    );
    assert_eq!(sent.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn long_replies_split_with_a_stable_key_for_each_part() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    let channel = channel();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let key = SecretString::from("pho_sk_offline".to_owned());
    let (api, sent) = message_api(StatusCode::ACCEPTED).await;
    let connection = Connection {
        bots: &bots,
        channel: &channel,
        client: &client,
        key: &key,
        api: &api,
        project_id: PROJECT,
        backlog_wait: Duration::from_millis(1),
    };
    let text = "x".repeat(MAX_CHUNK_UTF16 + 1);

    let result = connection.send_chunks("space/agent", "turn", &text).await;

    assert_eq!(result, Ok(()));
    let sent = sent.lock().unwrap();
    let keys: Vec<_> = sent.iter().map(|part| part.1.as_deref()).collect();
    assert_eq!(keys, [Some("turn:0"), Some("turn:1")]);
    assert_eq!(sent[1].2["content"]["text"], "x");
}

fn external(channel: ChannelId, sender: usize, index: usize) -> Incoming {
    Incoming {
        channel_id: channel,
        peer: format!("space-{sender}/agent"),
        sender: format!("sender-{sender}"),
        thread: None,
        message_id: format!("filler-{sender}-{index}"),
        text: "filler".to_owned(),
        reply_to: None,
        context: None,
    }
}

#[tokio::test]
async fn a_full_conversation_is_told_and_passed_but_a_full_backlog_is_retried() {
    let fixture = Fixture::new();
    fixture
        .native
        .hold
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let bots = fixture.bots();
    let channel = registered_channel(&fixture, &bots).await;
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let key = SecretString::from("pho_sk_offline".to_owned());
    let (api, sent) = message_api(StatusCode::ACCEPTED).await;
    let connection = Connection {
        bots: &bots,
        channel: &channel,
        client: &client,
        key: &key,
        api: &api,
        project_id: PROJECT,
        backlog_wait: Duration::from_millis(1),
    };
    let peer_event = || {
        let mut saturated = event("pho_msg_busy", OWNER, "hello");
        saturated["data"]["object"]["space"]["id"] = json!("space-0");
        saturated["data"]["object"]["receivedBy"]["id"] = json!("agent");
        saturated
    };
    // A running turn leaves the queue, so fill until the conversation actually refuses.
    for index in 0.. {
        let mut filler = external(channel.id, 0, index);
        filler.sender = OWNER.to_owned();
        if bots.accept(filler).await == Err(ErrorCode::ResourceExhausted) {
            break;
        }
    }
    let mut handled = None;

    let busy = connection
        .delivery(&peer_event(), &json!({"seq":4}), &mut handled)
        .await;

    assert_eq!(busy, Ok(()));
    assert_eq!(handled, Some(4));
    assert_eq!(
        sent.lock().unwrap()[0].1.as_deref(),
        Some("busy:pho_msg_busy")
    );

    fill_backlog(&bots, channel.id).await;
    let mut overflow = event("pho_msg_late", OWNER, "hello");
    overflow["data"]["object"]["space"]["id"] = json!("space-new");

    let retried = connection
        .delivery(&overflow, &json!({"seq":6}), &mut handled)
        .await;

    assert_eq!(retried, Err(ErrorCode::ResourceExhausted));
    assert_eq!(handled, Some(4));
    assert_eq!(bots.channel_cursor(channel.id).await, Ok(Some(4)));
}

async fn fill_backlog(bots: &Bots, channel: ChannelId) {
    for sender in 1.. {
        for index in 0.. {
            match bots.accept(external(channel, sender, index)).await {
                Ok(Admission::Backlogged) => return,
                Err(ErrorCode::ResourceExhausted) => break,
                Ok(Admission::Accepted) => {}
                Err(error) => panic!("unexpected admission failure: {error:?}"),
            }
        }
    }
}

type ServerSocket = tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;

/// Complete the handshake and return the subscribe frame.
async fn subscribe(socket: &mut ServerSocket) -> String {
    let open = r#"0{"sid":"s","pingInterval":25000,"pingTimeout":20000}"#;
    socket.send(WsMessage::Text(open.into())).await.unwrap();
    expect_frame(socket).await;
    socket
        .send(WsMessage::Text(r#"40{"sid":"n"}"#.into()))
        .await
        .unwrap();
    let subscribe = expect_frame(socket).await;
    socket
        .send(WsMessage::Text(SUBSCRIBED.into()))
        .await
        .unwrap();
    subscribe
}

#[tokio::test]
async fn a_full_backlog_replaces_the_connection_without_ending_the_receiver() {
    let fixture = Fixture::new();
    fixture
        .native
        .hold
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let bots = fixture.bots();
    let channel = registered_channel(&fixture, &bots).await;
    fill_backlog(&bots, channel.id).await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let api = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut first = tokio_tungstenite::accept_async(tcp).await.unwrap();
        let before = subscribe(&mut first).await;
        let refused = delivery(6, "pho_msg_late", OWNER);
        first.send(WsMessage::Text(refused.into())).await.unwrap();
        while first.next().await.is_some_and(|frame| frame.is_ok()) {}
        let (tcp, _) = listener.accept().await.unwrap();
        let mut second = tokio_tungstenite::accept_async(tcp).await.unwrap();
        let after = subscribe(&mut second).await;
        second.send(WsMessage::Text("41".into())).await.unwrap();
        (before, after)
    });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let key = SecretString::from("pho_sk_offline".to_owned());
    let connection = Connection {
        bots: &bots,
        channel: &channel,
        client: &client,
        key: &key,
        api: &api,
        project_id: PROJECT,
        backlog_wait: Duration::from_millis(1),
    };

    let ended = tokio::time::timeout(Duration::from_secs(10), connection.receive_all())
        .await
        .unwrap();

    assert_eq!(ended, Err(ErrorCode::BotIo));
    let (before, after) = tokio::time::timeout(Duration::from_secs(10), server)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(before, after);
    assert!(after.contains(r#""startSeq":0"#), "{after}");
    assert_eq!(bots.channel_cursor(channel.id).await, Ok(None));
}
