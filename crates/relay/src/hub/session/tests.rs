use tokio::sync::watch;
use tokio::time::timeout;

use super::*;
use crate::test_support::{Accepted, FakeApi, local};

const HELLO: &str = r#"{"type":"hello","client_id":"hub-test"}"#;

fn address() -> std::net::SocketAddr {
    "127.0.0.1:9".parse().expect("address")
}

async fn next(output: &mut mpsc::Receiver<(Uuid, Output)>) -> Output {
    timeout(Duration::from_secs(3), output.recv())
        .await
        .expect("session output")
        .expect("output channel open")
        .1
}

/// Channels of one session task: send peer messages on `inbound`, read its `output`.
struct Harness {
    inbound: mpsc::Sender<Frame>,
    output: mpsc::Receiver<(Uuid, Output)>,
    connections: mpsc::UnboundedReceiver<Accepted>,
}

impl Harness {
    /// Start an RPC session served by a fake API; `credit` makes it flow-controlled.
    fn start(guest: Option<Visitor>, credit: Option<credit::Outgoing>, refuse: bool) -> Self {
        let (api, connections) = FakeApi::new(refuse);
        let (inbound, inbound_rx) = mpsc::channel(4);
        let (output_tx, output) = mpsc::channel(4);
        let session = Session {
            id: Uuid::new_v4(),
            local: local(address(), Some(api)),
            visitor: guest,
            automation: false,
            inbound: inbound_rx,
            output: output_tx,
            credit,
        };
        tokio::spawn(run(
            session,
            SessionKind::Rpc,
            Bytes::from_static(HELLO.as_bytes()),
        ));
        Self {
            inbound,
            output,
            connections,
        }
    }

    /// Accept the session's connection, check its hello and answer it.
    async fn accept(&mut self) -> Accepted {
        let mut accepted = timeout(Duration::from_secs(3), self.connections.recv())
            .await
            .expect("connection in time")
            .expect("connection");
        assert_eq!(
            accepted.next().await,
            Some(Frame::Text(Bytes::from_static(HELLO.as_bytes())))
        );
        accepted.reply("server info").await;
        assert_eq!(
            next(&mut self.output).await,
            Output::Accept(Bytes::from_static(b"server info"))
        );
        accepted
    }
}

#[tokio::test]
async fn bridges_hello_replies_and_inbound_messages_then_reports_close() {
    let mut harness = Harness::start(None, None, false);
    let mut api = harness.accept().await;
    assert_eq!(api.guest, None);

    api.replies
        .send(Frame::Binary(Bytes::from_static(&[7])))
        .await
        .expect("binary");
    assert_eq!(
        next(&mut harness.output).await,
        Output::Message(PayloadKind::Binary, Bytes::from_static(&[7]))
    );
    harness
        .inbound
        .send(Frame::Text("request".into()))
        .await
        .expect("deliver");
    assert_eq!(api.next().await, Some(Frame::Text("request".into())));
    drop(api);

    assert_eq!(next(&mut harness.output).await, Output::Closed("closed"));
}

#[tokio::test]
async fn a_reply_larger_than_a_frame_ends_only_its_session_without_credit() {
    let mut harness = Harness::start(None, None, false);
    let api = harness.accept().await;
    api.replies
        .send(Frame::Binary(vec![0; wire::MAX_PAYLOAD + 1].into()))
        .await
        .expect("reply");
    assert_eq!(next(&mut harness.output).await, Output::Closed("overflow"));
}

#[tokio::test]
async fn realtime_voice_controls_pcm_and_playback_confirmation_traverse_hub() {
    use serde_json::json;

    let mut harness = Harness::start(None, None, false);
    let mut api = harness.accept().await;
    for params in [
        json!({
            "type":"request", "request_id":"start", "method":"voice.mode.set.request",
            "params":{"enabled":true,"agentId":"codex"}
        }),
        json!({
            "type":"event", "method":"voice.audio.chunk",
            "params":{"audio":"AAgACA==","format":"pcm;rate=16000","isLast":false}
        }),
        json!({"type":"event","method":"voice.audio.played","params":{"id":"audio-1"}}),
    ] {
        let frame = Frame::Text(params.to_string().into());
        harness.inbound.send(frame.clone()).await.expect("deliver");
        assert_eq!(api.next().await, Some(frame));
    }
    for params in [
        json!({
            "type":"response", "request_id":"start",
            "result":{"accepted":true,"enabled":true,"agentId":"codex"}
        }),
        json!({
            "type":"event", "method":"voice.audio.output", "params":{
                "id":"audio-1", "audio":"AAgACA==",
                "format":"audio/pcm;rate=24000;bits=16;channels=1",
                "isVoiceMode":true, "isLastChunk":true
            }
        }),
        json!({"type":"event","method":"voice.session.closed","params":{}}),
    ] {
        let text = params.to_string();
        api.replies
            .send(Frame::Text(text.clone().into()))
            .await
            .expect("reply");
        assert_eq!(
            next(&mut harness.output).await,
            Output::Message(PayloadKind::Text, Bytes::from(text))
        );
    }
    drop(harness.inbound);
    assert_eq!(api.next().await, None, "the local connection is closed");
}

fn session(local: Arc<Local>, output: mpsc::Sender<(Uuid, Output)>) -> Session {
    Session {
        id: Uuid::new_v4(),
        local,
        visitor: None,
        automation: false,
        inbound: mpsc::channel(1).1,
        output,
        credit: None,
    }
}

#[tokio::test]
async fn refuses_invalid_hello_unsupported_kinds_and_a_missing_api() {
    let (output, mut output_rx) = mpsc::channel(4);
    for (kind, hello, code) in [
        (SessionKind::Rpc, "{}", "invalid_hello"),
        (SessionKind::Rpc, HELLO, "unavailable"),
        (SessionKind::Unsupported, "", "unsupported"),
    ] {
        run(
            session(local(address(), None), output.clone()),
            kind,
            Bytes::from_static(hello.as_bytes()),
        )
        .await;
        assert_eq!(next(&mut output_rx).await, Output::Closed(code));
    }
}

#[tokio::test(start_paused = true)]
async fn an_api_that_never_answers_the_hello_times_out() {
    let (output, mut output_rx) = mpsc::channel(4);
    let (api, mut connections) = FakeApi::new(false);
    let task = tokio::spawn(run(
        session(local(address(), Some(api)), output),
        SessionKind::Rpc,
        Bytes::from_static(HELLO.as_bytes()),
    ));
    let _held = connections.recv().await.expect("connection");
    tokio::time::advance(FIRST_REPLY).await;
    task.await.expect("session");
    assert_eq!(
        output_rx.recv().await.expect("closed").1,
        Output::Closed("timeout")
    );
}

#[test]
fn payload_kinds_map_to_business_messages() {
    assert_eq!(
        message(PayloadKind::Text, Bytes::from_static(b"hi")),
        Ok(Frame::Text(Bytes::from_static(b"hi")))
    );
    assert_eq!(
        message(PayloadKind::Binary, Bytes::from_static(&[0xff])),
        Ok(Frame::Binary(Bytes::from_static(&[0xff])))
    );
    assert_eq!(
        message(PayloadKind::Text, Bytes::from_static(&[0xff])),
        Err("invalid_message")
    );
}

#[tokio::test]
async fn an_api_that_ends_before_or_after_its_reply_closes_the_session() {
    for after_reply in [false, true] {
        let mut harness = Harness::start(None, None, false);
        if after_reply {
            drop(harness.accept().await);
            assert_eq!(next(&mut harness.output).await, Output::Closed("closed"));
        } else {
            let mut api = harness.connections.recv().await.expect("connection");
            api.next().await.expect("hello");
            api.replies
                .send(Frame::Binary(Bytes::from_static(&[1])))
                .await
                .expect("binary first reply");
            assert_eq!(
                next(&mut harness.output).await,
                Output::Closed("unavailable")
            );
        }
    }
}

#[tokio::test]
async fn a_session_whose_link_is_gone_stops_after_the_handshake() {
    let (api, mut connections) = FakeApi::new(false);
    let (output, output_rx) = mpsc::channel(4);
    drop(output_rx);
    let task = tokio::spawn(run(
        session(local(address(), Some(api)), output),
        SessionKind::Rpc,
        Bytes::from_static(HELLO.as_bytes()),
    ));
    let mut accepted = connections.recv().await.expect("connection");
    accepted.next().await.expect("hello");
    accepted.reply("server info").await;
    timeout(Duration::from_secs(3), task)
        .await
        .expect("the bridge returns once nobody reads its output")
        .expect("session");
    assert_eq!(accepted.next().await, None, "local connection released");
}

fn visitor() -> Visitor {
    Visitor {
        user_id: Uuid::new_v4(),
        display_name: Some("Bob".to_owned()),
        username: Some("bob".to_owned()),
        group_id: Uuid::new_v4(),
        share_id: Uuid::new_v4(),
        resource_kind: model::share::ResourceKind::Node,
        resource_key: String::new(),
        permission: "operate".to_owned(),
    }
}

#[tokio::test]
async fn guest_sessions_connect_as_the_visitor_and_report_refusal_as_revoked() {
    let guest = visitor();
    let mut harness = Harness::start(Some(guest.clone()), None, false);
    assert_eq!(harness.accept().await.guest, Some(guest.clone()));

    let mut refused = Harness::start(Some(guest), None, true);
    assert_eq!(
        next(&mut refused.output).await,
        Output::Closed("share_revoked")
    );
    let mut owner = Harness::start(None, None, true);
    assert_eq!(next(&mut owner.output).await, Output::Closed("unavailable"));
}

#[tokio::test]
async fn credit_sessions_wait_for_the_peer_window_and_grant_what_they_delivered() {
    let (peer, credit) = watch::channel(0);
    let mut harness = Harness::start(None, Some(credit::Outgoing::new(credit, 65_536)), false);
    let mut api = harness.accept().await;

    // 40 KiB fit the 64 KiB window; the next 40 KiB wait for credit.
    for _ in 0..2 {
        api.replies
            .send(Frame::Binary(vec![1; 40 * 1024].into()))
            .await
            .expect("reply");
    }
    assert!(matches!(
        next(&mut harness.output).await,
        Output::Message(PayloadKind::Binary, body) if body.len() == 40 * 1024
    ));
    assert!(
        timeout(Duration::from_millis(100), harness.output.recv())
            .await
            .is_err(),
        "the second message exceeds the peer window"
    );
    peer.send_replace(16 * 1024);
    assert!(matches!(
        next(&mut harness.output).await,
        Output::Message(PayloadKind::Binary, _)
    ));

    // Every inbound message delivered locally is credited, cumulatively.
    for (chunk, credit) in [(1000, 1000), (24, 1024)] {
        harness
            .inbound
            .send(Frame::Binary(vec![0; chunk].into()))
            .await
            .expect("deliver");
        assert!(api.next().await.is_some());
        assert_eq!(next(&mut harness.output).await, Output::Credit(credit));
    }

    // A message larger than the whole peer window can never be admitted.
    api.replies
        .send(Frame::Binary(vec![0; 65_537].into()))
        .await
        .expect("reply");
    assert_eq!(next(&mut harness.output).await, Output::Closed("overflow"));
}

#[tokio::test]
async fn a_credit_session_waiting_for_credit_ends_when_its_route_is_dropped() {
    let (peer, credit) = watch::channel(0);
    let mut harness = Harness::start(None, Some(credit::Outgoing::new(credit, 65_536)), false);
    let api = harness.accept().await;
    api.replies
        .send(Frame::Binary(vec![1; 65_536].into()))
        .await
        .expect("reply");
    api.replies
        .send(Frame::Binary(vec![1; 1].into()))
        .await
        .expect("reply");
    assert!(matches!(
        next(&mut harness.output).await,
        Output::Message(..)
    ));
    drop(peer);
    assert_eq!(next(&mut harness.output).await, Output::Closed("closed"));
}
