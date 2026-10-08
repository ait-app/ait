use super::*;

async fn next(watch: &mut Watch) -> Change {
    tokio::time::timeout(Duration::from_secs(3), watch.changed())
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn observes_missing_creation_in_place_edit_atomic_replacement_removal_and_recreation() {
    let root = tempfile::tempdir().unwrap();
    let file = File::new(root.path().join("later/file.txt"));
    let mut watch = file.watch(Duration::from_millis(10)).await.unwrap();
    file.write(b"one").unwrap();
    assert_eq!(next(&mut watch).await, Change::Created);
    fs::write(file.path(), b"different length").unwrap();
    assert_eq!(next(&mut watch).await, Change::Modified);
    file.write(b"same length text").unwrap();
    assert_eq!(next(&mut watch).await, Change::Modified);
    fs::remove_file(file.path()).unwrap();
    assert_eq!(next(&mut watch).await, Change::Removed);
    file.write(b"back").unwrap();
    assert_eq!(next(&mut watch).await, Change::Created);
    assert!(
        tokio::time::timeout(Duration::from_millis(40), watch.changed())
            .await
            .is_err()
    );
    let task = watch.task.abort_handle();
    drop(watch);
    tokio::time::timeout(Duration::from_secs(3), async {
        while !task.is_finished() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[test]
fn observation_requires_a_running_runtime() {
    let file = File::new("unused-path");
    let mut future = Box::pin(file.watch(Duration::from_millis(10)));
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(matches!(
        future.as_mut().poll(&mut context),
        std::task::Poll::Ready(Err(Error::RuntimeUnavailable))
    ));
}

#[tokio::test]
async fn invalid_intervals_and_closed_tasks_report_errors() {
    let root = tempfile::tempdir().unwrap();
    let file = File::new(root.path().join("missing"));
    assert!(matches!(
        file.watch(Duration::ZERO).await,
        Err(Error::InvalidInterval)
    ));
    let mut watch = file.watch(Duration::from_millis(10)).await.unwrap();
    watch.task.abort();
    assert!(matches!(watch.changed().await, Err(Error::WatchClosed)));
}

#[tokio::test]
async fn inaccessible_metadata_is_reported_and_can_recover() {
    let root = tempfile::tempdir().unwrap();
    let parent = root.path().join("parent");
    let file = File::new(parent.join("child"));
    let mut watch = file.watch(Duration::from_millis(10)).await.unwrap();
    fs::write(&parent, b"not a directory").unwrap();
    assert!(matches!(next(&mut watch).await, Change::Unavailable(_)));
    fs::remove_file(&parent).unwrap();
    assert_eq!(next(&mut watch).await, Change::Removed);
    file.write(b"recovered").unwrap();
    assert_eq!(next(&mut watch).await, Change::Created);
}
