use std::sync::{Arc, Mutex as StdMutex};

use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::{WebSocketStream, accept_async, accept_hdr_async};

use super::*;

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
