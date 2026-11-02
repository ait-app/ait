use std::time::Duration;

use super::*;

#[tokio::test]
async fn three_short_jobs_execute_concurrently_and_a_fourth_is_rejected() {
    let runtime = runtime();
    let (started, mut starts) = tokio::sync::mpsc::channel(3);
    let mut completions = Vec::with_capacity(3);
    let mut tasks = Vec::with_capacity(3);
    for _ in 0..3 {
        let worker = runtime.clone();
        let started = started.clone();
        let (finish, gate) = std::sync::mpsc::channel();
        completions.push(finish);
        tasks.push(tokio::spawn(async move {
            worker
                .run_shared(Some(Arc::new(())), ErrorCode::RegistryIo, move |()| {
                    started.blocking_send(()).unwrap();
                    gate.recv_timeout(Duration::from_secs(5)).unwrap();
                    Ok(())
                })
                .await
        }));
    }
    drop(started);
    for _ in 0..3 {
        tokio::time::timeout(Duration::from_secs(5), starts.recv())
            .await
            .expect("all three jobs must start before any one completes")
            .expect("each admitted job reports its start");
    }
    assert_eq!(runtime.jobs.available_permits(), 0);
    assert_eq!(
        runtime
            .run_shared(Some(Arc::new(())), ErrorCode::RegistryIo, |()| Ok(()))
            .await,
        Err(ErrorCode::ResourceExhausted)
    );

    for finish in completions {
        finish.send(()).unwrap();
    }
    for task in tasks {
        assert_eq!(task.await.unwrap(), Ok(()));
    }
    assert_eq!(runtime.jobs.available_permits(), 3);
    assert!(runtime.tasks.is_empty());
}
