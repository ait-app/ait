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
async fn file_pollers_queue_fairly_instead_of_losing_contended_ticks() {
    let jobs = Arc::new(Semaphore::new(1));
    let held = jobs.clone().acquire_owned().await.unwrap();
    let cancel = CancellationToken::new();
    let server_cancel = CancellationToken::new();
    let (outbound, _receiver) = Outbound::new();
    let mut first = Box::pin(poll_permit(
        jobs.clone(),
        &cancel,
        &server_cancel,
        &outbound,
    ));
    let mut second = Box::pin(poll_permit(
        jobs.clone(),
        &cancel,
        &server_cancel,
        &outbound,
    ));
    assert_waiting(first.as_mut()).await;
    assert_waiting(second.as_mut()).await;
    drop(held);
    // Neither a later poll nor a foreground try-acquire may steal a queued permit.
    assert!(jobs.clone().try_acquire_owned().is_err());
    let first = first.await.unwrap();
    assert_waiting(second.as_mut()).await;
    drop(first);
    drop(second.await.unwrap());
    assert_eq!(jobs.available_permits(), 1);
}

#[tokio::test]
async fn released_shutdown_and_disconnected_file_pollers_leave_the_permit_queue() {
    for cancellation in ["subscription", "server", "outbound", "closed"] {
        let jobs = Arc::new(Semaphore::new(1));
        let held = jobs.clone().acquire_owned().await.unwrap();
        let cancel = CancellationToken::new();
        let server_cancel = CancellationToken::new();
        let (outbound, _receiver) = Outbound::new();
        let mut pending = Box::pin(poll_permit(
            jobs.clone(),
            &cancel,
            &server_cancel,
            &outbound,
        ));
        assert_waiting(pending.as_mut()).await;
        match cancellation {
            "subscription" => cancel.cancel(),
            "server" => server_cancel.cancel(),
            "outbound" => outbound.failure().cancel(),
            "closed" => jobs.close(),
            _ => unreachable!(),
        }
        assert!(pending.await.is_none());
        drop(held);
        if !jobs.is_closed() {
            assert!(jobs.try_acquire_owned().is_ok());
        }
    }
}
