use std::future::Future;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::{JoinHandle, LocalSet};
use tokio::time::{Instant, timeout};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{
    Callback, ErrorResponse, Request, Response,
};
use tokio_tungstenite::tungstenite::http::header::{AUTHORIZATION, RETRY_AFTER};
use tokio_tungstenite::tungstenite::http::{HeaderValue, StatusCode};
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::{WebSocketStream, accept_async, accept_hdr_async};
use tokio_util::sync::CancellationToken;

use super::{
    BACKOFF_CAP, Backoff, Exit, HEARTBEAT_TIMEOUT, Link, after_close, after_handshake, close_exit,
    close_frame, rejected_events,
};
use crate::config::Config;
use crate::hello::Offer;
use crate::outbox::{LastFrame, Outbox};
use crate::ports::Project;
use crate::runs::{Input, WritePolicy};
use crate::store::Store;
use crate::testing::MockHost;
use crate::wire::{Inbound, Person};

const RETRY: Exit = Exit::Retry {
    at_least: None,
    welcomed: true,
};
const RETRY_UNWELCOMED: Exit = Exit::Retry {
    at_least: None,
    welcomed: false,
};

fn events_frame() -> LastFrame {
    LastFrame {
        kind: "session.events".to_owned(),
        run_id: Some("run_a".to_owned()),
        epoch: Some("ep_1".to_owned()),
        range: Some((3, 7)),
    }
}

fn rejected_a() -> Input {
    Input::Rejected {
        run_id: "run_a".to_owned(),
        epoch: "ep_1".to_owned(),
        from: 3,
        to: 7,
    }
}

fn frame(code: u16) -> CloseFrame {
    CloseFrame {
        code: CloseCode::from(code),
        reason: "".into(),
    }
}

#[test]
fn revocation_and_replacement_close_codes_halt() {
    let revoked = after_close(4401);
    let replaced = after_close(4409);

    assert_eq!(revoked, Exit::Halt(super::Halt::Revoked));
    assert_eq!(replaced, Exit::Halt(super::Halt::Replaced));
    assert_ne!(
        revoked, replaced,
        "run() tells the two halts apart by reason"
    );
}

#[test]
fn other_close_codes_retry_with_plain_backoff() {
    for code in [4400, 4408, 4429, 1006, 1000, 1001, 1011] {
        assert_eq!(after_close(code), RETRY, "{code}");
    }
}

#[test]
fn handshake_401_halts_even_with_retry_after() {
    let retry_after = HeaderValue::from_static("5");

    let bare = after_handshake(StatusCode::UNAUTHORIZED, None);
    let with_header = after_handshake(StatusCode::UNAUTHORIZED, Some(&retry_after));

    assert_eq!(bare, Exit::Halt(super::Halt::Revoked));
    assert_eq!(with_header, Exit::Halt(super::Halt::Revoked));
}

#[test]
fn handshake_429_and_other_statuses_retry_as_network_failures() {
    let retry_after = HeaderValue::from_static("30");
    for status in [
        StatusCode::TOO_MANY_REQUESTS,
        StatusCode::NOT_FOUND,
        StatusCode::INTERNAL_SERVER_ERROR,
        StatusCode::BAD_GATEWAY,
    ] {
        assert_eq!(after_handshake(status, None), RETRY_UNWELCOMED, "{status}");
        assert_eq!(
            after_handshake(status, Some(&retry_after)),
            RETRY_UNWELCOMED,
            "only 503 honours Retry-After: {status}"
        );
    }
}

#[test]
fn handshake_503_waits_at_least_retry_after_seconds() {
    for (value, seconds) in [("7", 7), (" 12 ", 12), ("0", 0), ("120", 120)] {
        let header = HeaderValue::from_static(value);

        let exit = after_handshake(StatusCode::SERVICE_UNAVAILABLE, Some(&header));

        assert_eq!(
            exit,
            Exit::Retry {
                at_least: Some(Duration::from_secs(seconds)),
                welcomed: false,
            },
            "{value:?}"
        );
    }
}

#[test]
fn handshake_503_without_readable_retry_after_uses_plain_backoff() {
    let unreadable = [
        HeaderValue::from_static(""),
        HeaderValue::from_static("soon"),
        HeaderValue::from_static("-5"),
        HeaderValue::from_static("1.5"),
        HeaderValue::from_static("Wed, 21 Oct 2015 07:28:00 GMT"),
        HeaderValue::from_bytes(&[0xff, 0xfe]).expect("obs-text is a valid header value"),
    ];

    assert_eq!(
        after_handshake(StatusCode::SERVICE_UNAVAILABLE, None),
        RETRY_UNWELCOMED
    );
    for header in &unreadable {
        assert_eq!(
            after_handshake(StatusCode::SERVICE_UNAVAILABLE, Some(header)),
            RETRY_UNWELCOMED,
            "{header:?}"
        );
    }
}

#[test]
fn backoff_doubles_from_one_second_up_to_the_cap() {
    let ceilings = [1, 2, 4, 8, 16, 32, 60, 60, 60, 60];
    for _ in 0..100 {
        let mut backoff = Backoff::default();
        for ceiling in ceilings {
            let full = Duration::from_secs(ceiling);

            let wait = backoff.next(None);

            assert!(
                wait >= full / 2 && wait <= full,
                "{wait:?} outside [{:?}, {full:?}]",
                full / 2
            );
        }
    }
}

#[test]
fn backoff_jitter_spreads_over_half_to_full() {
    let waits: Vec<Duration> = (0..1000).map(|_| Backoff::default().next(None)).collect();

    let shortest = waits.iter().min().copied().unwrap_or_default();
    let longest = waits.iter().max().copied().unwrap_or_default();

    assert!(shortest < Duration::from_millis(600), "{shortest:?}");
    assert!(longest > Duration::from_millis(900), "{longest:?}");
}

#[test]
fn retry_after_raises_the_wait_but_never_past_the_cap() {
    let raised = Backoff::default().next(Some(Duration::from_secs(10)));
    let capped = Backoff::default().next(Some(Duration::from_mins(2)));
    let mut grown = Backoff::default();
    for _ in 0..5 {
        grown.next(None);
    }
    let below_backoff = grown.next(Some(Duration::from_secs(1)));

    assert_eq!(raised, Duration::from_secs(10));
    assert_eq!(capped, BACKOFF_CAP);
    assert!(
        below_backoff >= Duration::from_secs(16) && below_backoff <= Duration::from_secs(32),
        "a smaller Retry-After does not shorten the backoff: {below_backoff:?}"
    );
}

#[test]
fn reset_starts_again_from_one_second() {
    let mut backoff = Backoff::default();
    for _ in 0..8 {
        backoff.next(None);
    }

    backoff.reset();

    assert!(backoff.next(None) <= Duration::from_secs(1));
    assert!(backoff.next(None) <= Duration::from_secs(2));
}

#[test]
fn backoff_stays_capped_when_attempts_saturate() {
    let mut backoff = Backoff { attempt: u32::MAX };

    let first = backoff.next(None);
    let second = backoff.next(None);

    assert_eq!(backoff.attempt, u32::MAX);
    for wait in [first, second] {
        assert!(wait >= BACKOFF_CAP / 2 && wait <= BACKOFF_CAP, "{wait:?}");
    }
}

#[test]
fn rejected_session_events_name_their_run_epoch_and_range() {
    assert_eq!(rejected_events(&events_frame()), Some(rejected_a()));
}

#[test]
fn only_session_events_with_run_epoch_and_range_are_quarantined() {
    let cases = [
        LastFrame {
            kind: "run.status".to_owned(),
            ..events_frame()
        },
        LastFrame {
            kind: "session.unavailable".to_owned(),
            ..events_frame()
        },
        LastFrame {
            run_id: None,
            ..events_frame()
        },
        LastFrame {
            epoch: None,
            ..events_frame()
        },
        LastFrame {
            range: None,
            ..events_frame()
        },
        LastFrame {
            range: Some((0, -1)),
            ..events_frame()
        },
        LastFrame::default(),
    ];
    for last in cases {
        assert_eq!(rejected_events(&last), None, "{last:?}");
    }
}

#[test]
fn close_4400_after_session_events_reports_them_for_quarantine() {
    let (coordinator, mut inputs) = mpsc::unbounded_channel();
    let last = Arc::new(Mutex::new(events_frame()));

    let exit = close_exit(Some(&frame(4400)), &last, &coordinator);

    assert_eq!(exit, RETRY);
    assert_eq!(inputs.try_recv().ok(), Some(rejected_a()));
    assert!(inputs.try_recv().is_err());
}

#[test]
fn close_4400_after_a_status_frame_quarantines_nothing() {
    let (coordinator, mut inputs) = mpsc::unbounded_channel();
    let last = Arc::new(Mutex::new(LastFrame {
        kind: "run.status".to_owned(),
        run_id: Some("run_a".to_owned()),
        epoch: None,
        range: None,
    }));

    let exit = close_exit(Some(&frame(4400)), &last, &coordinator);

    assert_eq!(exit, RETRY);
    assert!(inputs.try_recv().is_err());
}

#[test]
fn other_closes_never_quarantine() {
    let (coordinator, mut inputs) = mpsc::unbounded_channel();
    let last = Arc::new(Mutex::new(events_frame()));

    let revoked = close_exit(Some(&frame(4401)), &last, &coordinator);
    let failed = close_exit(Some(&frame(1011)), &last, &coordinator);
    let abrupt = close_exit(None, &last, &coordinator);

    assert_eq!(revoked, after_close(4401));
    assert_eq!(failed, RETRY);
    assert_eq!(abrupt, after_close(1006), "no close frame counts as 1006");
    assert!(inputs.try_recv().is_err());
}

#[test]
fn close_frames_carry_the_code_and_an_empty_reason() {
    assert_eq!(
        close_frame(1000),
        Message::Close(Some(CloseFrame {
            code: CloseCode::Normal,
            reason: "".into(),
        }))
    );
    assert_eq!(close_frame(4401), Message::Close(Some(frame(4401))));
}

const ID: &str = "rt_0123456789abcdef0123456789abcdef";
const TOKEN: &str = "fedcba9876543210fedcba9876543210";
/// Real-time bound for one step against the fake Hub.
const WAIT: Duration = Duration::from_secs(5);
/// How long the fake Hub listens to prove that nothing was sent.
const QUIET: Duration = Duration::from_millis(300);
/// Longer than any backoff after a welcomed connection (at most one second).
const NO_REDIAL: Duration = Duration::from_millis(1500);
/// The link arms its timers in real time just before a test pauses the clock.
const ARMING_SLACK: Duration = Duration::from_secs(1);
/// Bound for reads while the clock is paused; only virtual time passes.
const VIRTUAL_WAIT: Duration = Duration::from_mins(10);

type Hub = WebSocketStream<TcpStream>;

/// What the fake Hub saw in one upgrade request.
#[derive(Debug, Clone, Default)]
struct Upgrade {
    path: String,
    query: Option<String>,
    authorization: Option<String>,
}

/// Records what the upgrade request carried, then accepts it.
struct Record(Arc<Mutex<Upgrade>>);

impl Callback for Record {
    fn on_request(self, request: &Request, response: Response) -> Result<Response, ErrorResponse> {
        *self.0.lock().expect("upgrade slot") = Upgrade {
            path: request.uri().path().to_owned(),
            query: request.uri().query().map(str::to_owned),
            authorization: request
                .headers()
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
        };
        Ok(response)
    }
}

/// Answers the upgrade request with an HTTP status instead of switching protocols.
struct Refuse {
    status: StatusCode,
    retry_after: Option<&'static str>,
}

impl Callback for Refuse {
    fn on_request(self, _: &Request, _: Response) -> Result<Response, ErrorResponse> {
        let mut builder = Response::builder().status(self.status);
        if let Some(seconds) = self.retry_after {
            builder = builder.header(RETRY_AFTER, seconds);
        }
        Err(builder.body(None).expect("a valid rejection"))
    }
}

/// A `Link` running against a fake Hub bound on a loopback port.
struct Harness {
    listener: TcpListener,
    host: MockHost,
    offer: Arc<RwLock<Offer>>,
    last: Arc<Mutex<LastFrame>>,
    inputs: mpsc::UnboundedReceiver<Input>,
    cancel: CancellationToken,
    task: JoinHandle<()>,
}

async fn start() -> Harness {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind a loopback port");
    let port = listener.local_addr().expect("a bound address").port();
    let config = Config::new(
        &format!("http://127.0.0.1:{port}"),
        ID.to_owned(),
        SecretString::from(TOKEN.to_owned()),
    )
    .expect("a valid runtime configuration");
    let host = MockHost::new();
    let offer = Arc::new(RwLock::new(Offer::default()));
    let last = Arc::new(Mutex::new(LastFrame::default()));
    let (coordinator, inputs) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();
    let link = Link {
        config,
        host: host.host(),
        store: Store::memory().expect("an in-memory store"),
        outbox: Outbox::default(),
        offer: Arc::clone(&offer),
        coordinator,
        name: "test-machine".to_owned(),
        policy: WritePolicy { loopback: true },
        cancel: cancel.clone(),
        last: Arc::clone(&last),
    };
    let task = tokio::task::spawn_local(link.run());
    Harness {
        listener,
        host,
        offer,
        last,
        inputs,
        cancel,
        task,
    }
}

impl Harness {
    async fn tcp(&self) -> TcpStream {
        let (stream, _) = timeout(WAIT, self.listener.accept())
            .await
            .expect("the runtime dials in time")
            .expect("accept the connection");
        stream
    }

    /// Accept the next upgrade and record its path and headers.
    async fn accept(&self) -> (Hub, Upgrade) {
        let stream = self.tcp().await;
        let seen = Arc::new(Mutex::new(Upgrade::default()));
        let record = Record(Arc::clone(&seen));
        let hub = timeout(WAIT, accept_hdr_async(stream, record))
            .await
            .expect("the upgrade completes in time")
            .expect("a WebSocket upgrade");
        let upgrade = seen.lock().expect("upgrade slot").clone();
        (hub, upgrade)
    }

    /// Accept upgrades until one completes, skipping dials the runtime abandoned.
    async fn accept_completed(&self) -> Hub {
        for _ in 0..5 {
            let stream = self.tcp().await;
            if let Ok(Ok(hub)) = timeout(WAIT, accept_async(stream)).await {
                return hub;
            }
        }
        panic!("no upgrade completed in five dials");
    }

    /// Answer the next upgrade with an HTTP status instead of switching protocols.
    async fn reject(&self, status: StatusCode, retry_after: Option<&'static str>) {
        let stream = self.tcp().await;
        let refuse = Refuse {
            status,
            retry_after,
        };
        let refused = timeout(WAIT, accept_hdr_async(stream, refuse))
            .await
            .expect("the rejection is written in time");
        assert!(refused.is_err(), "the upgrade was refused");
    }

    /// Accept, check the hello, welcome it and wait for `Input::Connected`.
    async fn connected(&mut self) -> Hub {
        let (mut hub, _) = self.accept().await;
        assert_eq!(head(&text(&mut hub, WAIT).await)["type"], "runtime.hello");
        send(&mut hub, &welcome()).await;
        assert_eq!(self.input().await, Input::Connected(owner()));
        hub
    }

    async fn input(&mut self) -> Input {
        timeout(WAIT, self.inputs.recv())
            .await
            .expect("an input in time")
            .expect("the link still holds its sender")
    }

    /// Wait for `Link::run` to return and collect the inputs it left behind.
    async fn finished(mut self) -> Vec<Input> {
        timeout(WAIT, &mut self.task)
            .await
            .expect("the link returns in time")
            .expect("the link task does not panic");
        let mut rest = Vec::new();
        while let Ok(input) = self.inputs.try_recv() {
            rest.push(input);
        }
        rest
    }

    async fn stop(self) -> Vec<Input> {
        self.cancel.cancel();
        self.finished().await
    }

    fn discoveries(&self) -> usize {
        self.host
            .executor
            .calls_of("provider.available.list.request")
            .len()
    }
}

/// `Link::run` is not `Send` (discovery borrows a `&dyn Fn`), so tests drive it on a `LocalSet`.
async fn local(body: impl Future<Output = ()>) {
    LocalSet::new().run_until(body).await;
}

fn owner() -> Person {
    Person {
        id: "github:1".to_owned(),
        login: Some("owner".to_owned()),
    }
}

fn welcome() -> Value {
    json!({"type": "runtime.welcome", "v": 1, "owner": {"id": "github:1", "login": "owner"}})
}

fn head(text: &str) -> Value {
    let head = text.split('\n').next().unwrap_or_default();
    serde_json::from_str(head).expect("a JSON frame head")
}

async fn send(hub: &mut Hub, frame: &Value) {
    send_text(hub, &frame.to_string()).await;
}

async fn send_text(hub: &mut Hub, text: &str) {
    hub.send(Message::Text(text.to_owned().into()))
        .await
        .expect("the fake Hub can write");
}

async fn text(hub: &mut Hub, within: Duration) -> String {
    let message = timeout(within, hub.next())
        .await
        .expect("a message in time")
        .expect("the connection stays open")
        .expect("a readable message");
    let Message::Text(text) = message else {
        panic!("expected a text message, got {message:?}");
    };
    text.as_str().to_owned()
}

/// Read until the runtime ends the connection; the close code, if it sent one.
async fn closed(hub: &mut Hub) -> Option<u16> {
    loop {
        let message = timeout(WAIT, hub.next())
            .await
            .expect("the runtime ends the connection in time");
        match message {
            Some(Ok(Message::Close(frame))) => return frame.map(|frame| u16::from(frame.code)),
            Some(Ok(_)) => {}
            Some(Err(_)) | None => return None,
        }
    }
}

#[tokio::test]
async fn hello_is_the_first_frame_and_carries_no_local_paths_or_secrets() {
    local(async {
        let harness = start().await;

        let (mut hub, upgrade) = harness.accept().await;
        let hello = text(&mut hub, WAIT).await;
        let frame = head(&hello);

        assert_eq!(upgrade.path, "/runtime");
        assert_eq!(upgrade.query, None, "the token never travels in the URL");
        assert_eq!(upgrade.authorization, Some(format!("Bearer {TOKEN}")));
        assert!(!hello.contains('\n'), "the hello is a head without a body");
        assert_eq!(frame["type"], "runtime.hello");
        assert_eq!(frame["v"], 1);
        assert_eq!(frame["runtime_id"], ID);
        assert_eq!(frame["session"], json!(["bonsai.session/1"]));
        assert_eq!(frame["name"], "test-machine");
        assert_eq!(frame["default_provider"], "claude");
        assert_eq!(frame["providers"][0]["id"], "claude");
        assert_eq!(frame["providers"][0]["bonsai_write"], true);
        assert_eq!(frame["providers"].as_array().map(Vec::len), Some(1));
        assert_eq!(frame["projects"][0]["git_remote"], "github.com/me/repo");
        assert!(!hello.contains("/tmp/repo"), "no local path: {hello}");
        assert!(!hello.contains(TOKEN), "no credential: {hello}");
        let announced = harness.offer.read().expect("offer lock").projects.len();
        assert_eq!(announced, 1, "dispatch validation sees the announced offer");
        harness.stop().await;
    })
    .await;
}

#[tokio::test]
async fn nothing_but_the_hello_is_sent_before_welcome() {
    local(async {
        let mut harness = start().await;
        let (mut hub, _) = harness.accept().await;
        text(&mut hub, WAIT).await;

        let early = timeout(QUIET, hub.next()).await;
        let early_input = harness.inputs.try_recv();
        send(&mut hub, &welcome()).await;
        let connected = harness.input().await;
        let unsolicited = timeout(QUIET, hub.next()).await;

        assert!(early.is_err(), "sent before welcome: {early:?}");
        assert!(
            early_input.is_err(),
            "connected before welcome: {early_input:?}"
        );
        assert_eq!(connected, Input::Connected(owner()));
        assert!(
            unsolicited.is_err(),
            "unsolicited after welcome: {unsolicited:?}"
        );
        harness.stop().await;
    })
    .await;
}

#[tokio::test]
async fn frames_from_the_hub_reach_the_coordinator() {
    local(async {
        let mut harness = start().await;
        let mut hub = harness.connected().await;

        send(&mut hub, &json!({"type": "run.query", "run_id": "run_a"})).await;
        let query = harness.input().await;
        send(&mut hub, &json!({"type": "run.cancel"})).await;
        let malformed = harness.input().await;
        send_text(&mut hub, "pong").await;
        send_text(&mut hub, "not json").await;
        send(
            &mut hub,
            &json!({"type": "hub.future_frame", "run_id": "run_a"}),
        )
        .await;
        let unknown = harness.input().await;

        assert_eq!(
            query,
            Input::Frame(Inbound::Query {
                run_id: "run_a".to_owned()
            })
        );
        assert_eq!(
            malformed,
            Input::Malformed {
                kind: "run.cancel".to_owned(),
                run_id: None
            }
        );
        assert_eq!(
            unknown,
            Input::Frame(Inbound::Unknown),
            "pong and a non-JSON head reach nobody; unknown types pass as Unknown"
        );
        harness.stop().await;
    })
    .await;
}

#[tokio::test]
async fn stopping_closes_with_1000_and_revokes_nothing() {
    local(async {
        let mut harness = start().await;
        let mut hub = harness.connected().await;

        harness.cancel.cancel();
        let code = closed(&mut hub).await;
        let rest = harness.finished().await;

        assert_eq!(code, Some(1000));
        assert_eq!(rest, Vec::new());
    })
    .await;
}

#[tokio::test]
async fn close_4401_halts_revokes_and_never_redials() {
    local(async {
        let mut harness = start().await;
        let mut hub = harness.connected().await;

        hub.send(close_frame(4401)).await.expect("send the close");
        let redial = timeout(NO_REDIAL, harness.listener.accept()).await;
        let rest = harness.finished().await;

        assert!(redial.is_err(), "redialled after 4401");
        assert_eq!(rest, vec![Input::Revoked]);
    })
    .await;
}

#[tokio::test]
async fn close_4409_halts_without_revoking() {
    local(async {
        let mut harness = start().await;
        let mut hub = harness.connected().await;

        hub.send(close_frame(4409)).await.expect("send the close");
        let redial = timeout(NO_REDIAL, harness.listener.accept()).await;
        let rest = harness.finished().await;

        assert!(redial.is_err(), "redialled after 4409");
        assert_eq!(
            rest,
            Vec::new(),
            "another process owns the runtime; its runs stay"
        );
    })
    .await;
}

#[tokio::test]
async fn close_1011_redials_with_a_fresh_hello() {
    local(async {
        let mut harness = start().await;
        let mut hub = harness.connected().await;

        hub.send(close_frame(1011)).await.expect("send the close");
        let (mut second, upgrade) = harness.accept().await;
        let hello = head(&text(&mut second, WAIT).await);

        assert_eq!(hello["type"], "runtime.hello");
        assert_eq!(hello["runtime_id"], ID);
        assert_eq!(upgrade.authorization, Some(format!("Bearer {TOKEN}")));
        assert_eq!(
            harness.discoveries(),
            2,
            "a welcomed connection rediscovers"
        );
        assert_eq!(harness.stop().await, Vec::new());
    })
    .await;
}

#[tokio::test]
async fn close_4400_after_session_events_quarantines_them_and_redials() {
    local(async {
        let mut harness = start().await;
        let mut hub = harness.connected().await;
        *harness.last.lock().expect("last frame") = events_frame();

        hub.send(close_frame(4400)).await.expect("send the close");
        let rejected = harness.input().await;
        let (mut second, _) = harness.accept().await;
        let hello = head(&text(&mut second, WAIT).await);

        assert_eq!(rejected, rejected_a());
        assert_eq!(hello["type"], "runtime.hello");
        harness.stop().await;
    })
    .await;
}

#[tokio::test]
async fn handshake_401_halts_and_revokes() {
    local(async {
        let harness = start().await;

        harness.reject(StatusCode::UNAUTHORIZED, None).await;
        let redial = timeout(NO_REDIAL, harness.listener.accept()).await;
        let rest = harness.finished().await;

        assert!(redial.is_err(), "redialled after a 401");
        assert_eq!(rest, vec![Input::Revoked]);
    })
    .await;
}

#[tokio::test]
async fn handshake_429_redials_like_a_network_failure() {
    local(async {
        let mut harness = start().await;

        harness.reject(StatusCode::TOO_MANY_REQUESTS, None).await;
        let rejected_at = Instant::now();
        let (mut hub, _) = harness.accept().await;
        let waited = rejected_at.elapsed();
        let hello = head(&text(&mut hub, WAIT).await);

        // The first backoff is at most 1 s; the margin covers the real clock's scheduling.
        assert!(
            waited <= Duration::from_millis(1_500),
            "first backoff is at most 1 s: {waited:?}"
        );
        assert_eq!(hello["type"], "runtime.hello");
        assert_eq!(
            harness.discoveries(),
            1,
            "an unwelcomed retry keeps the offer"
        );
        assert!(harness.inputs.try_recv().is_err(), "a 429 revokes nothing");
        harness.stop().await;
    })
    .await;
}

#[tokio::test]
async fn handshake_503_waits_for_retry_after_before_redialling() {
    local(async {
        let mut harness = start().await;

        harness
            .reject(StatusCode::SERVICE_UNAVAILABLE, Some("2"))
            .await;
        let rejected_at = Instant::now();
        let (mut hub, _) = harness.accept().await;
        let waited = rejected_at.elapsed();
        let hello = head(&text(&mut hub, WAIT).await);

        assert!(
            waited >= Duration::from_secs(2),
            "redialled after {waited:?}"
        );
        assert_eq!(hello["type"], "runtime.hello");
        assert!(harness.inputs.try_recv().is_err(), "a 503 revokes nothing");
        harness.stop().await;
    })
    .await;
}

#[tokio::test]
async fn cancelling_during_backoff_stops_the_link() {
    local(async {
        let harness = start().await;
        harness
            .reject(StatusCode::SERVICE_UNAVAILABLE, Some("30"))
            .await;

        let rest = harness.stop().await;

        assert_eq!(rest, Vec::new());
    })
    .await;
}

#[tokio::test]
async fn heartbeat_pings_and_a_silent_hub_is_dropped_and_redialled() {
    local(async {
        let mut harness = start().await;
        let mut hub = harness.connected().await;
        tokio::time::pause();
        let paused_at = Instant::now();

        let mut pings = 0;
        loop {
            let message = timeout(VIRTUAL_WAIT, hub.next())
                .await
                .expect("the link acts before the virtual deadline");
            match message {
                Some(Ok(Message::Text(text))) => {
                    assert_eq!(text.as_str(), "ping", "heartbeats are plain text");
                    pings += 1;
                }
                Some(Ok(other)) => panic!("unexpected {other:?}"),
                Some(Err(_)) | None => break,
            }
        }
        let silent_for = paused_at.elapsed();
        tokio::time::resume();
        let mut second = harness.accept_completed().await;
        let hello = head(&text(&mut second, WAIT).await);

        assert_eq!(
            pings, 2,
            "pings at 25 s and 50 s; at 75 s the link gives up"
        );
        assert!(
            silent_for >= HEARTBEAT_TIMEOUT,
            "dropped after {silent_for:?}"
        );
        assert!(
            silent_for < HEARTBEAT_TIMEOUT * 2,
            "dropped after {silent_for:?}"
        );
        assert_eq!(hello["type"], "runtime.hello");
        harness.stop().await;
    })
    .await;
}

#[tokio::test]
async fn close_4400_before_welcome_redials_without_quarantine() {
    local(async {
        let mut harness = start().await;
        let (mut hub, _) = harness.accept().await;
        text(&mut hub, WAIT).await;

        hub.send(close_frame(4400)).await.expect("send the close");
        let (mut second, _) = harness.accept().await;
        let hello = head(&text(&mut second, WAIT).await);

        assert_eq!(hello["type"], "runtime.hello");
        assert!(
            harness.inputs.try_recv().is_err(),
            "nothing to quarantine yet"
        );
        harness.stop().await;
    })
    .await;
}

#[tokio::test]
async fn a_hub_that_never_welcomes_is_abandoned_after_twenty_seconds() {
    local(async {
        let mut harness = start().await;
        let (mut hub, _) = harness.accept().await;
        text(&mut hub, WAIT).await;
        tokio::time::pause();
        let paused_at = Instant::now();

        let dropped = timeout(VIRTUAL_WAIT, hub.next())
            .await
            .expect("the link gives up before the virtual deadline");
        let waited = paused_at.elapsed();
        tokio::time::resume();

        // Reconnecting afterwards is covered by the 1011 test; with the clock paused here a
        // second connection's welcome timer would also fire at once.
        assert!(
            !matches!(dropped, Some(Ok(Message::Text(_)))),
            "nothing is sent while waiting for welcome: {dropped:?}"
        );
        assert!(
            waited + ARMING_SLACK >= Duration::from_secs(20),
            "gave up after {waited:?}"
        );
        assert!(harness.inputs.try_recv().is_err(), "never connected");
        harness.stop().await;
    })
    .await;
}

#[tokio::test]
async fn changed_projects_reconnect_with_a_new_hello_after_the_debounce() {
    local(async {
        let mut harness = start().await;
        let mut hub = harness.connected().await;
        harness
            .host
            .projects
            .projects
            .lock()
            .expect("projects lock")
            .push(Project {
                id: "prj_fedcba9876543210".to_owned(),
                name: "other".to_owned(),
                root: "/tmp/other".to_owned(),
                remote_url: Some("https://github.com/me/Other.git".to_owned()),
                branch: Some("dev".to_owned()),
            });
        tokio::time::pause();
        let paused_at = Instant::now();

        let code = loop {
            let message = timeout(VIRTUAL_WAIT, hub.next())
                .await
                .expect("the link acts before the virtual deadline");
            match message {
                Some(Ok(Message::Text(text))) if text.as_str() == "ping" => {
                    send_text(&mut hub, "pong").await;
                }
                Some(Ok(Message::Close(frame))) => break frame.map(|frame| u16::from(frame.code)),
                Some(Ok(other)) => panic!("unexpected {other:?}"),
                Some(Err(_)) | None => break None,
            }
        };
        let waited = paused_at.elapsed();
        tokio::time::resume();
        let mut second = harness.accept_completed().await;
        let hello = text(&mut second, WAIT).await;
        let frame = head(&hello);

        assert_eq!(
            code,
            Some(1000),
            "an orderly close, not a heartbeat timeout"
        );
        assert!(
            waited + ARMING_SLACK >= Duration::from_secs(90),
            "check at 60 s + 30 s debounce: {waited:?}"
        );
        assert!(
            waited < Duration::from_secs(150),
            "reconnected after {waited:?}"
        );
        assert_eq!(frame["projects"].as_array().map(Vec::len), Some(2));
        assert_eq!(frame["projects"][1]["git_remote"], "github.com/me/other");
        assert!(!hello.contains("/tmp/other"), "no local path: {hello}");
        harness.stop().await;
    })
    .await;
}

#[tokio::test]
async fn a_4400_on_a_new_connection_never_quarantines_the_previous_ones_frame() {
    local(async {
        let mut harness = start().await;
        let mut hub = harness.connected().await;
        // The previous connection last sent session events, then dropped.
        *harness.last.lock().expect("last frame") = events_frame();
        hub.send(close_frame(1011)).await.expect("send the close");

        let (mut second, _) = harness.accept().await;
        text(&mut second, WAIT).await;
        second
            .send(close_frame(4400))
            .await
            .expect("reject the hello");
        let (mut third, _) = harness.accept().await;
        let hello = head(&text(&mut third, WAIT).await);

        assert_eq!(hello["type"], "runtime.hello");
        while let Ok(input) = harness.inputs.try_recv() {
            assert!(
                !matches!(input, Input::Rejected { .. }),
                "quarantined a frame this connection never sent: {input:?}"
            );
        }
        harness.stop().await;
    })
    .await;
}

/// Answer pings until `until` (virtual time since `since`), calling `at_ping` with the elapsed
/// time of each; panics on any other frame, including a close.
async fn keep_alive(
    hub: &mut Hub,
    since: Instant,
    until: Duration,
    mut at_ping: impl FnMut(Duration),
) {
    loop {
        let left = until.saturating_sub(since.elapsed());
        match timeout(left, hub.next()).await {
            Err(_) => return,
            Ok(Some(Ok(Message::Text(text)))) if text.as_str() == "ping" => {
                at_ping(since.elapsed());
                send_text(hub, "pong").await;
            }
            Ok(other) => panic!("the link must stay connected, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn a_change_gone_before_the_confirming_check_never_reconnects() {
    local(async {
        let mut harness = start().await;
        let mut hub = harness.connected().await;
        let projects = Arc::clone(&harness.host.projects);
        let lists_before = projects.lists();
        projects
            .projects
            .lock()
            .expect("projects lock")
            .push(Project {
                id: "prj_fedcba9876543210".to_owned(),
                name: "other".to_owned(),
                root: "/tmp/other".to_owned(),
                remote_url: None,
                branch: None,
            });
        tokio::time::pause();
        let paused_at = Instant::now();

        // The check at 60 s sees the extra project; it is gone before the one at 90 s.
        let mut removed = false;
        keep_alive(&mut hub, paused_at, Duration::from_secs(200), |elapsed| {
            if !removed && elapsed >= Duration::from_secs(70) {
                projects.projects.lock().expect("projects lock").pop();
                removed = true;
            }
        })
        .await;
        tokio::time::resume();

        assert!(removed);
        assert!(
            projects.lists() - lists_before >= 3,
            "checked at 60, 90 and 120 s"
        );
        harness.stop().await;
    })
    .await;
}

#[tokio::test]
async fn a_slow_offer_check_never_stalls_inbound_frames_or_the_heartbeat() {
    local(async {
        let mut harness = start().await;
        let mut hub = harness.connected().await;
        let _gate = harness.host.projects.hold();
        tokio::time::pause();
        let paused_at = Instant::now();

        // The check starts at 60 s and never returns; the connection keeps working.
        keep_alive(&mut hub, paused_at, Duration::from_secs(80), |_| {}).await;
        let run_id = "r_00112233445566778899aabbccddeeff";
        send(&mut hub, &json!({"type": "run.query", "run_id": run_id})).await;
        let query = harness.input().await;
        keep_alive(&mut hub, paused_at, Duration::from_secs(200), |_| {}).await;
        tokio::time::resume();

        assert_eq!(
            query,
            Input::Frame(Inbound::Query {
                run_id: run_id.to_owned()
            })
        );
        harness.stop().await;
    })
    .await;
}

#[tokio::test]
async fn offer_checks_ask_for_providers_only_every_fifteen_minutes() {
    local(async {
        // Arrange: every provider query makes AIT's catalog rediscover all providers.
        let mut harness = start().await;
        let mut hub = harness.connected().await;
        let at_hello = harness.discoveries();
        tokio::time::pause();
        let paused_at = Instant::now();

        // Act
        keep_alive(
            &mut hub,
            paused_at,
            Duration::from_secs(14 * 60 + 30),
            |_| {},
        )
        .await;
        let before_fifteen = harness.discoveries();
        keep_alive(&mut hub, paused_at, Duration::from_mins(16), |_| {}).await;
        let after_fifteen = harness.discoveries();
        tokio::time::resume();

        // Assert
        assert_eq!(at_hello, 1, "the hello discovers the providers");
        assert_eq!(
            before_fifteen, at_hello,
            "project checks leave providers alone"
        );
        assert_eq!(
            after_fifteen,
            at_hello + 1,
            "providers are rechecked at 15 min"
        );
        assert!(
            harness.host.projects.lists() >= 15,
            "projects are checked every minute"
        );
        harness.stop().await;
    })
    .await;
}
