use super::*;
use crate::test_support::{Fixture, WORKSPACE, setup};
use domain::bot::{Turn, TurnStatus};
use model::bot::Store;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Notify;

fn outbox(fixture: &Fixture) -> (ChannelId, Vec<Turn>) {
    let mut state = fixture.state();
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    let channel = ChannelId::generate();
    state
        .execute(
            "bot.channel.create.request",
            json!({"workspaceId":WORKSPACE,
        "channelId":channel,"enabled":true,"config":{"kind":"feishu","appId":"app",
        "secretRef":"env:AIT_SERVER_CREDENTIAL_TEST"}}),
        )
        .unwrap();
    let mut turns = Vec::new();
    for message in ["first", "second"] {
        state
            .accept(Incoming {
                channel_id: channel,
                peer: "chat".into(),
                sender: "alice".into(),
                thread: None,
                message_id: message.into(),
                text: message.into(),
                reply_to: Some(message.into()),
                sender_type: None,
                passive: false,
            })
            .unwrap();
        let run = state.ready().unwrap().pop().unwrap();
        state
            .progress(
                run.turn.id,
                TurnStatus::Completed,
                Some(message.into()),
                None,
            )
            .unwrap();
        state.finished(run.turn.conversation_id);
        turns.push(state.turn(run.turn.id).unwrap());
    }
    (channel, turns)
}

fn delivery(fixture: &Fixture, turn: &Turn) -> DeliveryStatus {
    fixture
        .store
        .turn(turn.conversation_id, turn.id)
        .unwrap()
        .delivery
}

#[tokio::test(start_paused = true)]
async fn shutdown_finishes_the_in_flight_send_without_claiming_another_reply() {
    let fixture = Fixture::new();
    let (channel, turns) = outbox(&fixture);
    let bots = fixture.bots();
    let cancel = CancellationToken::new();
    let stop = cancel.child_token();
    let started = Notify::new();
    let finish = Notify::new();
    let sends = AtomicUsize::new(0);
    let sending = deliver(&bots, channel, &stop, |_, _| async {
        sends.fetch_add(1, Ordering::SeqCst);
        started.notify_one();
        finish.notified().await;
        Ok(())
    });
    let channel = supervise(&cancel, &stop, std::future::pending(), sending);
    tokio::pin!(channel);
    tokio::select! {
        () = started.notified() => {},
        result = &mut channel => panic!("channel ended before the send started: {result:?}"),
    }
    cancel.cancel();
    assert!(futures_util::poll!(&mut channel).is_pending());
    tokio::time::advance(Duration::from_millis(9_999)).await; // virtual time
    assert!(futures_util::poll!(&mut channel).is_pending());
    finish.notify_one();
    channel.await.unwrap();
    assert_eq!(sends.load(Ordering::SeqCst), 1);
    assert_eq!(delivery(&fixture, &turns[0]), DeliveryStatus::Delivered);
    assert_eq!(delivery(&fixture, &turns[1]), DeliveryStatus::Pending);
    bots.shutdown().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn shutdown_limits_an_ambiguous_send_to_ten_seconds() {
    let fixture = Fixture::new();
    let (channel, turns) = outbox(&fixture);
    let bots = fixture.bots();
    let cancel = CancellationToken::new();
    let started = Notify::new();
    let sending = deliver(&bots, channel, &cancel, |_, _| async {
        started.notify_one();
        std::future::pending().await
    });
    tokio::pin!(sending);
    tokio::select! {
        () = started.notified() => {},
        result = &mut sending => panic!("delivery ended before the send started: {result:?}"),
    }
    cancel.cancel();
    let stopped = tokio::time::Instant::now();
    assert!(futures_util::poll!(&mut sending).is_pending());
    tokio::time::advance(Duration::from_millis(9_999)).await; // virtual time
    assert!(futures_util::poll!(&mut sending).is_pending());
    tokio::time::advance(Duration::from_millis(1)).await; // virtual time
    sending.await.unwrap();
    assert_eq!(stopped.elapsed(), Duration::from_secs(10));
    assert_eq!(delivery(&fixture, &turns[0]), DeliveryStatus::Uncertain);
    assert_eq!(delivery(&fixture, &turns[1]), DeliveryStatus::Pending);
    bots.shutdown().await.unwrap();
}

#[tokio::test]
async fn stopping_before_delivery_preserves_unclaimed_replies() {
    let fixture = Fixture::new();
    let (channel, turns) = outbox(&fixture);
    let bots = fixture.bots();
    let cancel = CancellationToken::new();
    cancel.cancel();
    deliver(&bots, channel, &cancel, |_, _| async {
        panic!("a stopped delivery loop must not contact the platform")
    })
    .await
    .unwrap();
    assert!(
        turns
            .iter()
            .all(|turn| delivery(&fixture, turn) == DeliveryStatus::Pending)
    );
    bots.shutdown().await.unwrap();
}

#[test]
fn replies_split_on_unicode_boundaries_and_bound_utf16_length() {
    let text = "a日😀".repeat(2000);
    let pieces = chunks(&text, 3500);
    assert_eq!(pieces.concat(), text);
    assert!(
        pieces
            .iter()
            .all(|piece| piece.encode_utf16().count() <= 3500)
    );
    assert!(chunks("", 3500).is_empty());
}

#[tokio::test]
async fn oversized_http_responses_are_rejected_without_echoing_the_body() {
    use axum::{Router, routing::get};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route("/", get(|| async { "sensitive".repeat(150_000) })),
        )
        .await
        .unwrap();
    });
    let response = reqwest::get(format!("http://{address}")).await.unwrap();
    assert_eq!(
        json_response(response).await,
        Err(ErrorCode::ResourceExhausted)
    );
    server.abort();
}

#[tokio::test]
async fn a_failed_receiver_ends_delivery_through_its_stop_token() {
    let cancel = CancellationToken::new();
    let stop = cancel.child_token();
    let sending = async {
        stop.cancelled().await;
        Ok(())
    };
    let result = supervise(&cancel, &stop, async { Err(ErrorCode::BotIo) }, sending).await;
    assert_eq!(result, Err(ErrorCode::BotIo));
}

#[tokio::test]
async fn a_failed_delivery_loop_ends_the_channel() {
    let cancel = CancellationToken::new();
    let stop = cancel.child_token();
    let receiving = std::future::pending();
    let result = supervise(&cancel, &stop, receiving, async { Err(ErrorCode::BotIo) }).await;
    assert_eq!(result, Err(ErrorCode::BotIo));
}
