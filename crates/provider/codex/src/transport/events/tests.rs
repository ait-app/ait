use super::*;
use serde_json::json;

#[tokio::test]
async fn spill_write_failures_are_explicit() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("readonly");
    std::fs::write(&path, "").unwrap();
    let mut queue = Queue {
        spool: Some(Spool {
            file: File::open(path).unwrap(),
            read_position: 0,
            remaining: 0,
        }),
        ..Queue::default()
    };
    assert_eq!(
        queue.push(json!({"event":"must retain"})).await,
        Err(AgentSessionError::Failed)
    );
}

async fn next(queue: &mut Queue) -> Value {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            match queue.poll().unwrap() {
                Poll::Ready(Some(event)) => return event,
                Poll::Ready(None) => panic!("expected retained event"),
                Poll::Pending => tokio::task::yield_now().await,
            }
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn appending_while_a_spill_read_is_pending_preserves_order_and_releases_storage() {
    let mut queue = Queue::default();
    for index in 0..512 {
        queue.push(json!({"index":index})).await.unwrap();
    }
    assert_eq!(queue.memory.len(), MEMORY_EVENTS);
    for index in 0..MEMORY_EVENTS {
        assert_eq!(next(&mut queue).await["index"], index);
    }
    assert_eq!(queue.poll().unwrap(), Poll::Pending);
    for index in 512..1024 {
        queue.push(json!({"index":index})).await.unwrap();
    }
    for index in MEMORY_EVENTS..1024 {
        assert_eq!(next(&mut queue).await["index"], index);
    }
    assert_eq!(queue.poll().unwrap(), Poll::Ready(None));
    assert!(queue.spool.is_none());
    assert!(queue.reading.is_none());
    queue.push(json!({"next":"burst"})).await.unwrap();
    assert_eq!(next(&mut queue).await, json!({"next":"burst"}));
}

#[tokio::test]
async fn corrupt_spill_data_fails_instead_of_skipping_an_event() {
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(b"not-json\n").unwrap();
    let mut queue = Queue {
        spool: Some(Spool {
            file,
            read_position: 0,
            remaining: 1,
        }),
        ..Queue::default()
    };
    assert_eq!(queue.poll().unwrap(), Poll::Pending);
    let failure = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            match queue.poll() {
                Err(error) => return error,
                Ok(Poll::Pending) => tokio::task::yield_now().await,
                other => panic!("expected a spill read failure, got {other:?}"),
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(failure, AgentSessionError::Failed);
}
