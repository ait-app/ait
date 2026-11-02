use std::future::{Future, poll_fn};
use std::task::Poll;

use super::*;

#[tokio::test]
async fn checkout_jobs_are_serialized_and_do_not_occupy_catalog_admission() {
    let runtime = runtime();
    let service = Arc::new(Mutex::new(0_u8));
    let (started, start) = tokio::sync::oneshot::channel();
    let (release, released) = std::sync::mpsc::channel();
    let task_runtime = runtime.clone();
    let checkout = service.clone();
    let task = tokio::spawn(async move {
        task_runtime
            .run_checkout(Some(checkout), ErrorCode::ProjectIo, move |value| {
                started.send(()).unwrap();
                released.recv().unwrap();
                *value += 1;
                Ok(*value)
            })
            .await
    });
    start.await.unwrap();
    let mut queued =
        Box::pin(
            runtime.run_checkout(Some(service.clone()), ErrorCode::ProjectIo, |value| {
                *value += 1;
                Ok(*value)
            }),
        );
    poll_fn(|context| {
        assert!(queued.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    assert_eq!(runtime.jobs.available_permits(), 3);
    assert_eq!(runtime.checkout_jobs.available_permits(), 0);
    assert_eq!(
        runtime
            .run(
                Some(Arc::new(Mutex::new(()))),
                ErrorCode::RegistryIo,
                |()| Ok(7)
            )
            .await,
        Ok(7)
    );
    release.send(()).unwrap();
    assert_eq!(task.await.unwrap(), Ok(1));
    assert_eq!(queued.await, Ok(2));
    assert_eq!(runtime.checkout_jobs.available_permits(), 1);
}

#[tokio::test]
async fn shutdown_cancels_checkout_waiters_and_missing_services_do_not_queue() {
    let runtime = runtime();
    let held = runtime.checkout_jobs.clone().acquire_owned().await.unwrap();
    assert_eq!(
        runtime
            .run_checkout::<(), ()>(None, ErrorCode::ProjectIo, |()| Ok(()))
            .await,
        Err(ErrorCode::UnsupportedCapability)
    );
    let mut queued = Box::pin(runtime.run_checkout(
        Some(Arc::new(Mutex::new(()))),
        ErrorCode::ProjectIo,
        |()| -> Result<(), ErrorCode> { panic!("shutdown prevents queued Git operations") },
    ));
    poll_fn(|context| {
        assert!(queued.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    runtime.cancellation.cancel();
    assert_eq!(queued.await, Err(ErrorCode::ServerDraining));
    assert_eq!(runtime.tasks.len(), 0);
    drop(held);
    assert_eq!(runtime.checkout_jobs.available_permits(), 1);
}

#[tokio::test]
async fn dropped_checkout_response_retains_its_tracked_blocking_job() {
    let runtime = runtime();
    let service = Arc::new(Mutex::new(0_u8));
    let (started, start) = tokio::sync::oneshot::channel();
    let (release, released) = std::sync::mpsc::channel();
    let task_runtime = runtime.clone();
    let checkout = service.clone();
    let task = tokio::spawn(async move {
        task_runtime
            .run_checkout(Some(checkout), ErrorCode::ProjectIo, move |value| {
                started.send(()).unwrap();
                released.recv().unwrap();
                *value += 1;
                Ok(())
            })
            .await
    });
    start.await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(runtime.checkout_jobs.available_permits(), 0);
    assert_eq!(runtime.jobs.available_permits(), 3);
    assert_eq!(runtime.tasks.len(), 1);
    release.send(()).unwrap();
    runtime.tasks.close();
    tokio::time::timeout(std::time::Duration::from_secs(5), runtime.tasks.wait())
        .await
        .unwrap();
    assert_eq!(*service.lock().unwrap(), 1);
    assert_eq!(runtime.checkout_jobs.available_permits(), 1);
}
