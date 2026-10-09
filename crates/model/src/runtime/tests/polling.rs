use std::future::{Future, poll_fn};
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::Duration;

use crate::ErrorCode;
use crate::tests::runtime;

#[tokio::test]
async fn saturated_checkout_polling_does_not_consume_foreground_admission() {
    let runtime = runtime();
    let held = runtime
        .checkout_poll_jobs
        .clone()
        .acquire_owned()
        .await
        .unwrap();
    assert!(
        runtime
            .checkout_poll_jobs
            .clone()
            .try_acquire_owned()
            .is_err()
    );
    let mut queued = Box::pin(runtime.checkout_poll_jobs.clone().acquire_owned());
    poll_fn(|context| {
        assert!(queued.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;

    let service = Arc::new(Mutex::new(0_u8));
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        runtime.run(Some(service.clone()), ErrorCode::RegistryIo, |value| {
            *value += 1;
            Ok(*value)
        }),
    )
    .await
    .expect("unrelated foreground service proceeds while all poll permits are occupied");
    assert_eq!(result, Ok(1));
    assert_eq!(*service.lock().unwrap(), 1);
    assert_eq!(runtime.checkout_poll_jobs.available_permits(), 0);
    assert_eq!(runtime.jobs.available_permits(), 1);

    drop(held);
    drop(queued.await.unwrap());
    assert_eq!(runtime.checkout_poll_jobs.available_permits(), 1);
}

#[tokio::test]
async fn directory_reads_wait_on_their_own_budget_without_rejecting_foreground_requests() {
    let runtime = runtime();
    let directory = Arc::new(Mutex::new(0_u8));
    let (started, start) = tokio::sync::oneshot::channel();
    let (release, released) = std::sync::mpsc::channel::<()>();
    let reader = runtime.clone();
    let read_directory = directory.clone();
    let read = tokio::spawn(async move {
        reader
            .run_directory_read(Some(read_directory), ErrorCode::RegistryIo, move |value| {
                started.send(()).unwrap();
                released.recv().unwrap();
                *value += 1;
                Ok(*value)
            })
            .await
    });
    start.await.unwrap();
    assert_eq!(runtime.directory_poll_jobs.available_permits(), 0);
    assert_eq!(runtime.jobs.available_permits(), 1);

    let foreground = Arc::new(Mutex::new(0_u8));
    let admitted = runtime
        .run(Some(foreground.clone()), ErrorCode::RegistryIo, |value| {
            *value += 1;
            Ok(*value)
        })
        .await;
    assert_eq!(
        admitted,
        Ok(1),
        "a running directory read must not exhaust admission"
    );

    let queued_runtime = runtime.clone();
    let queued_directory = directory.clone();
    let queued = tokio::spawn(async move {
        queued_runtime
            .run_directory_read(Some(queued_directory), ErrorCode::RegistryIo, |value| {
                *value += 1;
                Ok(*value)
            })
            .await
    });
    tokio::task::yield_now().await;
    assert!(
        !queued.is_finished(),
        "a second directory read waits instead of failing"
    );

    release.send(()).unwrap();
    assert_eq!(read.await.unwrap(), Ok(1));
    assert_eq!(queued.await.unwrap(), Ok(2));
    assert_eq!(runtime.directory_poll_jobs.available_permits(), 1);
    assert_eq!(runtime.jobs.available_permits(), 1);
}

#[tokio::test]
async fn shutdown_cancels_directory_reads_waiting_for_their_budget() {
    let runtime = runtime();
    let _held = runtime
        .directory_poll_jobs
        .clone()
        .acquire_owned()
        .await
        .unwrap();
    let waiting_runtime = runtime.clone();
    let waiting = tokio::spawn(async move {
        waiting_runtime
            .run_directory_read(
                Some(Arc::new(Mutex::new(()))),
                ErrorCode::RegistryIo,
                |()| -> Result<(), ErrorCode> { panic!("shutdown must prevent this read") },
            )
            .await
    });
    tokio::task::yield_now().await;
    runtime.cancellation.cancel();
    assert_eq!(waiting.await.unwrap(), Err(ErrorCode::ServerDraining));
    assert_eq!(
        runtime
            .run_directory_read::<(), ()>(None, ErrorCode::RegistryIo, |()| Ok(()))
            .await,
        Err(ErrorCode::UnsupportedCapability)
    );
}
