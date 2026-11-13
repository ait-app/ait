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
            return Err(Error::Storage);
        }
        *self.records.lock().unwrap() = records.to_vec();
        Ok(())
    }
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
