use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::task::Poll;

use super::*;

async fn assert_waiting(mut future: Pin<&mut impl Future>) {
    poll_fn(|context| {
        assert!(future.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
}

#[tokio::test]
async fn json_backpressure_retains_the_socket_until_the_active_write_releases_capacity() {
    let (queue, mut receiver) = Outbound::new();
    queue.send(&message(MAX_QUEUE_BYTES / 2)).unwrap();
    let in_flight = receiver.recv().await.unwrap();
    let large = message(MAX_QUEUE_BYTES / 2);
    let mut waiting = Box::pin(queue.send_waiting(&large));
    assert_waiting(waiting.as_mut()).await;
    assert!(!queue.failure().is_cancelled());
    drop(in_flight);
    waiting.await.unwrap();
    assert!(matches!(
        receiver.recv().await.unwrap().message,
        Frame::Text(_)
    ));
    assert!(!queue.failure().is_cancelled());
    assert_eq!(queue.bytes.available_permits(), MAX_QUEUE_BYTES);
}

#[tokio::test]
async fn json_waits_for_message_slots_on_legacy_and_fair_connections() {
    let (queue, mut receiver) = Outbound::new();
    for _ in 0..MAX_QUEUE_MESSAGES {
        queue.send(&message(1)).unwrap();
    }
    let next = message(2);
    let available = queue.bytes.available_permits();
    let mut waiting = Box::pin(queue.send_waiting(&next));
    assert_waiting(waiting.as_mut()).await;
    drop(waiting);
    assert_eq!(queue.bytes.available_permits(), available);
    let mut waiting = Box::pin(queue.send_waiting(&next));
    assert_waiting(waiting.as_mut()).await;
    drop(receiver.recv().await.unwrap());
    waiting.await.unwrap();
    assert!(!queue.failure().is_cancelled());

    let (lanes, mut receiver) = Outbound::fair();
    for _ in 0..MAX_QUEUE_MESSAGES {
        lanes[2].send(&message(1)).unwrap();
    }
    let mut waiting = Box::pin(lanes[2].send_waiting(&next));
    assert_waiting(waiting.as_mut()).await;
    let in_flight = receiver.recv().await.unwrap();
    assert_waiting(waiting.as_mut()).await;
    drop(in_flight);
    waiting.await.unwrap();
    assert!(!lanes[0].failure().is_cancelled());
}

#[tokio::test]
async fn closed_or_cancelled_json_waits_release_reserved_output() {
    for close_receiver in [false, true] {
        let (queue, mut receiver) = Outbound::new();
        queue.binary(vec![1; MAX_QUEUE_BYTES]).await.unwrap();
        let in_flight = receiver.recv().await.unwrap();
        let next = message(1);
        let mut waiting = Box::pin(queue.send_waiting(&next));
        assert_waiting(waiting.as_mut()).await;
        if close_receiver {
            drop(receiver);
        } else {
            queue.failure().cancel();
        }
        assert!(matches!(waiting.await, Err(QueueError::Full)));
        drop(in_flight);
        assert_eq!(queue.bytes.available_permits(), MAX_QUEUE_BYTES);
    }
}

#[tokio::test]
async fn oversized_json_is_rejected_without_waiting_or_cancelling_the_connection() {
    let (queue, mut receiver) = Outbound::new();
    assert!(matches!(
        queue.send_waiting(&message(MAX_QUEUE_BYTES)).await,
        Err(QueueError::Full)
    ));
    assert!(!queue.failure().is_cancelled());
    queue.send_waiting(&message(1)).await.unwrap();
    drop(receiver.recv().await.unwrap());
    assert_eq!(queue.bytes.available_permits(), MAX_QUEUE_BYTES);
}

#[tokio::test]
async fn a_large_json_waiter_does_not_reserve_the_bytes_needed_by_control_responses() {
    let (lanes, mut receiver) = Outbound::fair();
    lanes[2].send(&message(MAX_QUEUE_BYTES / 2)).unwrap();
    let next = message(MAX_QUEUE_BYTES / 2);
    let mut waiting = Box::pin(lanes[2].send_waiting(&next));
    assert_waiting(waiting.as_mut()).await;
    lanes[0]
        .respond("ping".to_owned(), Ok(json!({"nonce":"alive"})))
        .unwrap();
    let ping = receiver.recv().await.unwrap();
    let Frame::Text(text) = &ping.message else {
        panic!("expected the control response")
    };
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(text).unwrap()["request_id"],
        "ping"
    );
    drop(ping);
    assert_waiting(waiting.as_mut()).await;
    drop(receiver.recv().await.unwrap());
    waiting.await.unwrap();
    drop(receiver.recv().await.unwrap());
    assert!(!lanes[0].failure().is_cancelled());
    assert_eq!(lanes[0].bytes.available_permits(), MAX_QUEUE_BYTES);
}
