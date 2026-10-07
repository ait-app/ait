use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test]
async fn shared_services_use_existing_admission_and_preserve_business_errors() {
    let runtime = runtime();
    let service = Arc::new(AtomicUsize::new(0));
    assert_eq!(
        runtime
            .run_shared::<AtomicUsize, ()>(None, ErrorCode::RegistryIo, |_| Ok(()))
            .await,
        Err(ErrorCode::UnsupportedCapability)
    );
    let permit = runtime.jobs.clone().acquire_owned().await.unwrap();
    assert_eq!(
        runtime
            .run_shared(Some(service.clone()), ErrorCode::RegistryIo, |_| Ok(()))
            .await,
        Err(ErrorCode::ResourceExhausted)
    );
    drop(permit);
    assert_eq!(
        runtime
            .run_shared(Some(service.clone()), ErrorCode::RegistryIo, |counter| {
                counter.fetch_add(1, Ordering::SeqCst);
                Err::<(), _>(ErrorCode::InvalidMessage)
            })
            .await,
        Err(ErrorCode::InvalidMessage)
    );
    assert_eq!(service.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.jobs.available_permits(), 1);
    runtime.cancellation.cancel();
    assert_eq!(
        runtime
            .run_shared(Some(service.clone()), ErrorCode::RegistryIo, |_| Ok(()))
            .await,
        Err(ErrorCode::ServerDraining)
    );
    assert_eq!(service.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn dropped_shared_response_retains_the_owner_and_tracked_job() {
    let runtime = runtime();
    let service = Arc::new(AtomicUsize::new(0));
    let (started, start) = tokio::sync::oneshot::channel();
    let (release, released) = std::sync::mpsc::channel();
    let task_runtime = runtime.clone();
    let owner = service.clone();
    let task = tokio::spawn(async move {
        task_runtime
            .run_shared(Some(owner), ErrorCode::RegistryIo, move |counter| {
                started.send(()).unwrap();
                released.recv().unwrap();
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .await
    });
    start.await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(runtime.jobs.available_permits(), 0);
    assert_eq!(runtime.tasks.len(), 1);
    release.send(()).unwrap();
    runtime.tasks.close();
    tokio::time::timeout(std::time::Duration::from_secs(5), runtime.tasks.wait())
        .await
        .unwrap();
    assert_eq!(service.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.jobs.available_permits(), 1);
}

#[tokio::test]
async fn shared_service_panics_return_the_safe_failure_and_release_admission() {
    let runtime = runtime();
    let result = runtime
        .run_shared(
            Some(Arc::new(())),
            ErrorCode::RegistryIo,
            |()| -> Result<(), ErrorCode> {
                panic!("inject shared service panic");
            },
        )
        .await;
    assert_eq!(result, Err(ErrorCode::RegistryIo));
    assert_eq!(runtime.jobs.available_permits(), 1);
}
