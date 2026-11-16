//! An occurrence that cannot be persisted holds back only its own schedule.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use domain::schedule::{RunStatus, Schedule};

use super::*;
use crate::engine::tests::input;

/// A store that refuses to persist a schedule once one of its occurrences has ended.
#[derive(Debug, Default, Clone)]
struct Flaky {
    records: Arc<Mutex<Vec<Schedule>>>,
    failing: Arc<Mutex<String>>,
    failed: Arc<tokio::sync::Notify>,
}

impl Store for Flaky {
    fn load(&self) -> Result<Vec<Schedule>, Error> {
        Ok(self.records.lock().unwrap().clone())
    }

    fn save(&mut self, records: &[Schedule]) -> Result<(), Error> {
        let failing = self.failing.lock().unwrap().clone();
        let refused = records.iter().any(|record| {
            record.id == failing
                && record
                    .runs
                    .iter()
                    .any(|run| run.status != RunStatus::Running)
        });
        if refused {
            self.failed.notify_one();
            return Err(Error::Storage);
        }
        *self.records.lock().unwrap() = records.to_vec();
        Ok(())
    }
}

#[tokio::test]
async fn an_unsettled_occurrence_does_not_block_another_manual_run() {
    use super::controlled::*;

    let store = Flaky::default();
    let (runner, mut receive) = Controlled::new();
    let service = Schedules::spawn(Box::new(store.clone()), runner).unwrap();
    let blocked = create(&service).await;
    let other = create(&service).await;
    *store.failing.lock().unwrap() = blocked.as_str().unwrap().to_owned();
    let pending = run(&service, &blocked);
    started(&mut receive)
        .await
        .complete
        .send(Outcome::default())
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), store.failed.notified())
        .await
        .unwrap();
    assert!(!pending.is_finished());

    let other_run = run(&service, &other);
    tokio::task::yield_now().await;
    assert!(
        !other_run.is_finished(),
        "another schedule must be admitted"
    );
    let accepted = started(&mut receive).await;
    assert_eq!(accepted.schedule.id, other.as_str().unwrap());
    accepted.complete.send(Outcome::default()).unwrap();
    settled(other_run).await.unwrap();
    assert_eq!(
        settled(run(&service, &blocked)).await.unwrap_err(),
        Error::Storage
    );

    store.failing.lock().unwrap().clear();
    settled(pending).await.unwrap();
    service.shutdown().await.unwrap();
}

/// A runner that counts its invocations.
#[derive(Debug, Default)]
struct Counting {
    calls: AtomicUsize,
}

#[async_trait::async_trait]
impl Runner for Counting {
    async fn resolve(
        &self,
        _: String,
        target: domain::schedule::Target,
    ) -> Result<domain::schedule::Target, Error> {
        Ok(target)
    }

    async fn run(&self, _: Schedule, _: String, _: Progress, _: CancellationToken) -> Outcome {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Outcome {
            output: Some("done".into()),
            ..Outcome::default()
        }
    }
}

#[tokio::test]
async fn an_unsettled_occurrence_holds_back_only_its_own_schedule() {
    let store = Flaky::default();
    let runner = Arc::new(Counting::default());
    let service = Schedules::spawn(Box::new(store.clone()), runner.clone()).unwrap();
    let mut first = input();
    first["cadence"] = json!({"type":"every","everyMs":1_000});
    first["runOnCreate"] = json!(false);
    let blocked = service
        .execute("schedule.create.request", first)
        .await
        .unwrap()["schedule"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut second = input();
    second["name"] = json!("second");
    second["cadence"] = json!({"type":"every","everyMs":4_000});
    second["runOnCreate"] = json!(false);
    service
        .execute("schedule.create.request", second)
        .await
        .unwrap();
    *store.failing.lock().unwrap() = blocked;

    // The blocked schedule ends its occurrence first; the other one still runs.
    tokio::time::timeout(Duration::from_secs(10), async {
        while runner.calls.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    service.shutdown().await.unwrap();
}
