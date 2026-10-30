use std::sync::{Arc, Mutex as StdMutex};

use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::{WebSocketStream, accept_async, accept_hdr_async};

use super::*;

mod center_calls;

const HELLO: &str = r#"{"type":"hello","client_id":"hub-test"}"#;
const TICKET: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const RESUME: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

type Center = WebSocketStream<TcpStream>;

fn frame(header: &Value, payload: &[u8]) -> Message {
    let header = serde_json::to_vec(header).expect("header");
    let mut bytes = u16::try_from(header.len())
        .expect("short header")
        .to_be_bytes()
        .to_vec();
    bytes.extend_from_slice(&header);
    bytes.extend_from_slice(payload);
    Message::Binary(bytes.into())
}

/// Next non-ack frame from the Host as (header, payload).
async fn next(center: &mut Center) -> (Value, Vec<u8>) {
    loop {
        let message = timeout(Duration::from_secs(3), center.next())
            .await
            .expect("frame in time")
            .expect("open")
            .expect("healthy");
        let Message::Binary(bytes) = message else {
            continue;
        };
        let length = usize::from(u16::from_be_bytes([bytes[0], bytes[1]]));
        let header: Value = serde_json::from_slice(&bytes[2..2 + length]).expect("header");
        if header["t"] != "ack" {
            return (header, bytes[2 + length..].to_vec());
        }
    }
}

/// Accept one hub upgrade, recording its bearer credential.
#[expect(
    clippy::result_large_err,
    reason = "tungstenite's handshake callback fixes the error type"
)]
async fn accept(listener: &TcpListener) -> (Center, String) {
    let (tcp, _) = timeout(Duration::from_secs(5), listener.accept())
        .await
        .expect("dial in time")
        .expect("accept");
    let seen = Arc::new(StdMutex::new(String::new()));
    let record = seen.clone();
    let socket = accept_hdr_async(tcp, move |request: &Request, response: Response| {
        assert_eq!(request.uri().path(), "/v1/hub");
        *record.lock().expect("record") = request.headers()["authorization"]
            .to_str()
            .expect("ascii")
            .to_owned();
        Ok(response)
    })
    .await
    .expect("upgrade");
    let credential = seen.lock().expect("record").clone();
    (socket, credential)
}

fn welcome(link: Uuid, resumed: bool, recv: u64) -> Value {
    let mut welcome = json!({"t":"welcome","v":1,"link":link,"resumed":resumed,"recv":recv,
        "node_id":Uuid::new_v4(),"grace_seconds":120,"ping_seconds":20,"max_payload":1_048_576});
    if !resumed {
        welcome["resume_token"] = json!(RESUME);
    }
    welcome
}

async fn daemon(listener: TcpListener) -> Vec<String> {
    let (tcp, _) = listener.accept().await.expect("accept");
    let mut socket = accept_async(tcp).await.expect("upgrade");
    let hello = socket.next().await.expect("hello").expect("frame");
    assert_eq!(hello.to_text().expect("text"), HELLO);
    socket
        .send(Message::Text("server info".into()))
        .await
        .expect("info");
    socket
        .send(Message::Binary(Bytes::from_static(&[1])))
        .await
        .expect("event");
    let mut received = Vec::new();
    while let Some(Ok(message)) = socket.next().await {
        if let Message::Text(text) = message {
            received.push(text.to_string());
            if received.len() == 2 {
                socket.close(None).await.expect("close");
                break;
            }
        }
    }
    received
}

async fn connector() -> (crate::Connector, Connector, TcpListener, TcpListener) {
    let center = TcpListener::bind("127.0.0.1:0").await.expect("bind center");
    let local = TcpListener::bind("127.0.0.1:0").await.expect("bind local");
    let relay = crate::Connector::new(
        local.local_addr().expect("local address"),
        SecretString::from("local-token"),
        Uuid::new_v4().to_string(),
        Uuid::new_v4().to_string(),
    );
    let hub = relay.hub().clone();
    hub.start(Grant {
        center_url: format!("http://{}", center.local_addr().expect("center address")),
        hub_ticket: SecretString::from(TICKET),
        node_session_id: Uuid::new_v4(),
    })
    .await
    .expect("start");
    (relay, hub, center, local)
}

#[tokio::test]
async fn sessions_survive_link_loss_and_unacknowledged_frames_are_replayed() {
    let (_relay, hub, center, local) = connector().await;
    let daemon = tokio::spawn(daemon(local));
    let (mut first, credential) = accept(&center).await;
    assert_eq!(credential, format!("Bearer {TICKET}"));
    assert_eq!(
        next(&mut first).await.0,
        json!({"t":"hello","v":1,"resume":null})
    );
    let link = Uuid::new_v4();
    first
        .send(frame(&welcome(link, false, 0), b""))
        .await
        .expect("welcome");
    let s = Uuid::new_v4();
    first
        .send(frame(
            &json!({"t":"open","seq":1,"s":s,"from":Uuid::new_v4()}),
            HELLO.as_bytes(),
        ))
        .await
        .expect("open");
    let (accepted, info) = next(&mut first).await;
    assert_eq!(accepted, json!({"t":"accept","seq":1,"s":s}));
    assert_eq!(info, b"server info");
    assert_eq!(
        next(&mut first).await,
        (json!({"t":"msg","seq":2,"s":s,"pt":"binary"}), vec![1])
    );
    first
        .send(frame(
            &json!({"t":"msg","seq":2,"s":s,"pt":"text"}),
            b"before",
        ))
        .await
        .expect("msg");
    assert_eq!(hub.status().await.link, Some(link));

    // The center only stored the accept before the connection dropped.
    drop(first);
    let (mut second, credential) = accept(&center).await;
    assert_eq!(credential, format!("Bearer {RESUME}"));
    assert_eq!(
        next(&mut second).await.0,
        json!({"t":"hello","v":1,"resume":{"link":link,"recv":2}})
    );
    second
        .send(frame(&welcome(link, true, 1), b""))
        .await
        .expect("resume");
    assert_eq!(
        next(&mut second).await,
        (json!({"t":"msg","seq":2,"s":s,"pt":"binary"}), vec![1])
    );
    // A replayed center frame is acknowledged but not delivered twice.
    second
        .send(frame(
            &json!({"t":"msg","seq":2,"s":s,"pt":"text"}),
            b"before",
        ))
        .await
        .expect("replay");
    second
        .send(frame(
            &json!({"t":"msg","seq":3,"s":s,"pt":"text"}),
            b"after",
        ))
        .await
        .expect("msg");
    assert_eq!(daemon.await.expect("daemon"), ["before", "after"]);
    let (closed, _) = next(&mut second).await;
    assert_eq!(closed, json!({"t":"close","seq":3,"s":s,"code":"closed"}));
    assert!(hub.status().await.online);

    hub.stop().await;
    assert_eq!(hub.status().await, Status::default());
}

#[tokio::test]
#[expect(
    clippy::result_large_err,
    reason = "tungstenite's handshake callback fixes the error type"
)]
async fn refused_resume_ends_the_link_until_a_new_grant() {
    let (_relay, hub, center, _local) = connector().await;
    let (mut first, _) = accept(&center).await;
    next(&mut first).await;
    first
        .send(frame(&welcome(Uuid::new_v4(), false, 0), b""))
        .await
        .expect("welcome");
    next_status(&hub, |status| status.online).await;
    drop(first);

    let (tcp, _) = center.accept().await.expect("redial");
    let refused = accept_hdr_async(tcp, |_: &Request, _: Response| {
        let mut response = ErrorResponse::new(None);
        *response.status_mut() = StatusCode::UNAUTHORIZED;
        Err(response)
    })
    .await;
    assert!(refused.is_err());

    let status = next_status(&hub, |status| !status.connecting && !status.online).await;
    assert_eq!(
        status.error.as_deref(),
        Some("invalid relay center or ticket")
    );
    assert_eq!(status.link, None);
}

#[tokio::test]
async fn malformed_grants_never_dial() {
    let relay = crate::Connector::new(
        "127.0.0.1:9".parse().expect("address"),
        SecretString::from("local-token"),
        Uuid::new_v4().to_string(),
        Uuid::new_v4().to_string(),
    );
    let hub = relay.hub().clone();
    for (center_url, ticket) in [
        ("https://example.com", "short"),
        ("http://example.com", TICKET),
    ] {
        let grant = Grant {
            center_url: center_url.to_owned(),
            hub_ticket: SecretString::from(ticket),
            node_session_id: Uuid::new_v4(),
        };
        assert!(matches!(hub.start(grant).await, Err(Error::InvalidGrant)));
    }
    relay.begin_shutdown();
    let grant = Grant {
        center_url: "https://example.com".to_owned(),
        hub_ticket: SecretString::from(TICKET),
        node_session_id: Uuid::new_v4(),
    };
    assert!(matches!(hub.start(grant).await, Err(Error::Transport)));
}

async fn next_status(hub: &Connector, ready: impl Fn(&Status) -> bool) -> Status {
    timeout(Duration::from_secs(5), async {
        loop {
            let status = hub.status().await;
            if ready(&status) {
                return status;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("status in time")
}

#[tokio::test]
async fn a_replaced_link_reports_why_and_yields_until_restarted() {
    let (_relay, hub, center, _local) = connector().await;
    let (mut socket, _) = accept(&center).await;
    next(&mut socket).await;
    socket
        .send(frame(&welcome(Uuid::new_v4(), false, 0), b""))
        .await
        .expect("welcome");
    next_status(&hub, |status| status.online).await;
    assert!(!hub.yielding().await);

    socket
        .send(frame(&json!({"t":"error","code":"replaced"}), b""))
        .await
        .expect("error");

    let status = next_status(&hub, |status| !status.online && !status.connecting).await;
    assert_eq!(status.rejected.as_deref(), Some("replaced"));
    assert_eq!(
        status.error.as_deref(),
        Some("hub rejected the link: replaced")
    );
    assert!(hub.yielding().await, "another process now serves this node");
    hub.start(Grant {
        center_url: format!("http://{}", center.local_addr().expect("center address")),
        hub_ticket: SecretString::from(TICKET),
        node_session_id: Uuid::new_v4(),
    })
    .await
    .expect("an explicit grant takes the link back");
    assert!(!hub.yielding().await);
}

#[tokio::test]
async fn rejections_before_welcome_end_the_link_without_yielding() {
    let (_relay, hub, center, _local) = connector().await;
    let (mut socket, _) = accept(&center).await;
    next(&mut socket).await;
    socket
        .send(frame(&json!({"t":"error","code":"unauthorized"}), b""))
        .await
        .expect("error");

    let status = next_status(&hub, |status| !status.connecting).await;
    assert_eq!(status.rejected.as_deref(), Some("unauthorized"));
    assert!(!status.online);
    assert!(!hub.yielding().await);
}

/// A local daemon that accepts sessions and reads their hello but never answers.
fn silent_daemon(listener: TcpListener) {
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((tcp, _)) = listener.accept().await {
            if let Ok(mut socket) = accept_async(tcp).await {
                let _ = socket.next().await;
                held.push(socket);
            }
        }
    });
}

/// Accept the first connection and complete a fresh welcome with `grace_seconds`.
async fn online(hub: &Connector, center: &TcpListener, grace_seconds: u64) -> Center {
    let (mut socket, _) = accept(center).await;
    next(&mut socket).await;
    let mut welcome = welcome(Uuid::new_v4(), false, 0);
    welcome["grace_seconds"] = json!(grace_seconds);
    socket.send(frame(&welcome, b"")).await.expect("welcome");
    next_status(hub, |status| status.online).await;
    socket
}

fn open(seq: u64, s: Uuid, hello: &[u8]) -> Message {
    frame(
        &json!({"t":"open","seq":seq,"s":s,"from":Uuid::new_v4()}),
        hello,
    )
}

#[tokio::test]
async fn center_frames_drive_session_refusals_and_link_control() {
    let (_relay, hub, center, local) = connector().await;
    silent_daemon(local);
    let mut socket = online(&hub, &center, 120).await;

    socket
        .send(Message::Ping(Bytes::from_static(b"center")))
        .await
        .expect("ping");
    let pong = timeout(Duration::from_secs(3), async {
        loop {
            if let Some(Ok(Message::Pong(body))) = socket.next().await {
                return body;
            }
        }
    })
    .await
    .expect("pong");
    assert_eq!(pong.as_ref(), b"center");
    socket
        .send(frame(&json!({"t":"ack","a":0}), b""))
        .await
        .expect("ack");

    let invalid = Uuid::new_v4();
    socket.send(open(1, invalid, b"{}")).await.expect("open");
    assert_eq!(
        next(&mut socket).await.0,
        json!({"t":"close","seq":1,"s":invalid,"code":"invalid_hello"})
    );

    // Sixteen sessions wait for the silent daemon; the next one is refused.
    let pending: Vec<Uuid> = (0..16).map(|_| Uuid::new_v4()).collect();
    for (seq, s) in (2..).zip(&pending) {
        socket
            .send(open(seq, *s, HELLO.as_bytes()))
            .await
            .expect("open");
    }
    // Replayed opens of a live session are ignored.
    socket
        .send(open(18, pending[0], HELLO.as_bytes()))
        .await
        .expect("duplicate");
    let refused = Uuid::new_v4();
    socket
        .send(open(19, refused, HELLO.as_bytes()))
        .await
        .expect("open");
    let (closed, _) = next(&mut socket).await;
    assert_eq!(closed["s"], json!(refused));
    assert_eq!(closed["code"], "capacity");

    let text = |seq: u64, s: Uuid| frame(&json!({"t":"msg","seq":seq,"s":s,"pt":"text"}), &[0xff]);
    socket
        .send(text(20, Uuid::new_v4()))
        .await
        .expect("unknown session");
    socket
        .send(text(21, pending[1]))
        .await
        .expect("invalid text");
    let (closed, _) = next(&mut socket).await;
    assert_eq!(closed["s"], json!(pending[1]));
    assert_eq!(closed["code"], "invalid_message");
    socket
        .send(frame(
            &json!({"t":"close","seq":22,"s":pending[2],"code":"done"}),
            b"",
        ))
        .await
        .expect("close");

    // A second welcome on a live link is a protocol violation.
    socket
        .send(frame(&welcome(Uuid::new_v4(), false, 0), b""))
        .await
        .expect("welcome");
    let status = next_status(&hub, |status| !status.online && !status.connecting).await;
    assert_eq!(status.error.as_deref(), Some("relay protocol rejected"));
}

#[tokio::test]
async fn text_frames_from_the_center_end_the_link() {
    let (_relay, hub, center, _local) = connector().await;
    let mut socket = online(&hub, &center, 120).await;
    socket
        .send(Message::Text("not a hub frame".into()))
        .await
        .expect("text");
    let status = next_status(&hub, |status| !status.online && !status.connecting).await;
    assert_eq!(status.error.as_deref(), Some("relay protocol rejected"));
}

#[tokio::test]
async fn a_link_without_grace_left_ends_after_a_clean_close() {
    let (_relay, hub, center, _local) = connector().await;
    // The grace period equals the safety margin, so no resume is attempted.
    let mut socket = online(&hub, &center, 5).await;
    socket.close(None).await.expect("close");
    let status = next_status(&hub, |status| !status.online && !status.connecting).await;
    assert_eq!(status.error.as_deref(), Some("relay transport unavailable"));
    assert_eq!(status.link, None);
}

#[tokio::test]
async fn an_unreachable_center_ends_the_first_attempt() {
    let (_relay, hub, center, _local) = connector().await;
    drop(center);
    let status = next_status(&hub, |status| !status.connecting).await;
    assert_eq!(status.error.as_deref(), Some("relay transport unavailable"));
    assert!(!status.online);
}

#[tokio::test]
async fn frames_before_the_welcome_are_rejected() {
    let (_relay, hub, center, _local) = connector().await;
    let (mut socket, _) = accept(&center).await;
    next(&mut socket).await;
    socket
        .send(frame(&json!({"t":"ack","a":0}), b""))
        .await
        .expect("ack");
    let status = next_status(&hub, |status| !status.connecting).await;
    assert_eq!(status.error.as_deref(), Some("relay protocol rejected"));
}

#[tokio::test]
async fn stop_ends_an_online_link_and_its_late_result_is_ignored() {
    let (_relay, hub, center, _local) = connector().await;
    let (mut socket, _) = accept(&center).await;
    next(&mut socket).await;
    // Hop-local pings before the welcome are answered or ignored.
    socket
        .send(Message::Ping(Bytes::from_static(b"early")))
        .await
        .expect("ping");
    socket
        .send(Message::Pong(Bytes::from_static(b"stray")))
        .await
        .expect("pong");
    socket
        .send(frame(&welcome(Uuid::new_v4(), false, 0), b""))
        .await
        .expect("welcome");
    next_status(&hub, |status| status.online).await;

    hub.stop().await;
    let closed = timeout(Duration::from_secs(3), async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Close(_)) | Err(_)) | None => return,
                Some(Ok(_)) => {}
            }
        }
    })
    .await;
    assert!(closed.is_ok(), "the center connection is closed");
    sleep(Duration::from_millis(100)).await;
    assert_eq!(hub.status().await, Status::default());
}

#[tokio::test]
async fn disconnect_discards_previous_owner_yield() {
    let (_relay, hub, center, _local) = connector().await;
    let (mut socket, _) = accept(&center).await;
    next(&mut socket).await;
    socket
        .send(frame(&welcome(Uuid::new_v4(), false, 0), b""))
        .await
        .unwrap();
    next_status(&hub, |status| status.online).await;
    socket
        .send(frame(&json!({"t":"error","code":"replaced"}), b""))
        .await
        .unwrap();
    next_status(&hub, |status| !status.online && !status.connecting).await;
    assert!(hub.yielding().await);
    hub.stop().await;
    assert!(
        !hub.yielding().await,
        "a subsequent fresh login must not inherit the old grant's five-minute Hub suppression"
    );
}

#[tokio::test]
async fn share_status_frames_and_guest_opens_reach_the_daemon() {
    let (_relay, hub, center, local) = connector().await;
    let mut status = model::hub::Center::share_status(&hub);
    let mut socket = online(&hub, &center, 120).await;

    let share = Uuid::new_v4();
    socket
        .send(frame(
            &json!({"t":"share_status","seq":1}),
            json!({"revision":3,"accepted":[],"rejected":[
                {"workspaceKey":"ws_1","organizationId":share,"code":"revoked"}],"orgs":[]})
            .to_string()
            .as_bytes(),
        ))
        .await
        .expect("status");
    timeout(Duration::from_secs(3), status.changed())
        .await
        .expect("status in time")
        .expect("connector alive");
    let outcome = status.borrow_and_update().clone().expect("outcome");
    assert_eq!(outcome.revision, Some(3));
    assert_eq!(outcome.rejected[0].code, "revoked");

    let visitor = json!({"user_id":Uuid::new_v4(),"display_name":"Bob","organization_id":share,
        "share_id":share,"workspace_key":"ws_1","permission":"operate"});
    socket
        .send(frame(
            &json!({"t":"open","seq":2,"s":Uuid::new_v4(),"from":Uuid::new_v4(),"visitor":visitor}),
            HELLO.as_bytes(),
        ))
        .await
        .expect("guest open");
    let guest = accept_guest(&local).await.expect("guest header");
    assert_eq!(
        (guest.share_id, guest.workspace_key.as_str()),
        (share, "ws_1")
    );
}

/// Accept one bridged daemon connection and return its guest visitor, if any.
#[expect(
    clippy::result_large_err,
    reason = "tungstenite's handshake callback fixes the error type"
)]
async fn accept_guest(local: &TcpListener) -> Option<Visitor> {
    let (tcp, _) = timeout(Duration::from_secs(3), local.accept())
        .await
        .expect("bridge dials")
        .expect("accept");
    let seen = Arc::new(StdMutex::new(None));
    let record = seen.clone();
    let _socket = accept_hdr_async(tcp, move |request: &Request, response: Response| {
        *record.lock().expect("record") = request
            .headers()
            .get(model::share::GUEST_HEADER)
            .and_then(|value| value.to_str().ok())
            .and_then(Visitor::decode);
        Ok(response)
    })
    .await
    .expect("upgrade");
    seen.lock().expect("record").clone()
}
