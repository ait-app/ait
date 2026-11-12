//! Every durably settled occurrence is reported to the runner once when it settles, and again
//! whenever the worker opens within the re-offer window after it ended.

use std::sync::Mutex;

use domain::schedule::{Run, RunStatus};

use super::*;

/// One reported settlement: schedule id, run id, status and error.
type Settled = (String, String, RunStatus, Option<String>);

/// Runner that finishes every occurrence and records each reported settlement.
#[derive(Debug, Default)]
struct Observed {
    settled: Mutex<Vec<Settled>>,
}

#[async_trait::async_trait]
impl Runner for Observed {
    async fn resolve(
        &self,
        _: String,
        target: domain::schedule::Target,
    ) -> Result<domain::schedule::Target, Error> {
        Ok(target)
    }

    async fn run(&self, _: Schedule, _: String, _: Progress, _: CancellationToken) -> Outcome {
        Outcome {
            output: Some("done".into()),
            ..Outcome::default()
        }
    }

    fn settled(&self, schedule: &Schedule, run: &Run) {
        self.settled.lock().unwrap().push((
            schedule.id.clone(),
            run.id.clone(),
            run.status,
            run.error.clone(),
        ));
    }
}

#[tokio::test]
async fn each_settled_run_is_reported_once_after_it_is_saved() {
    let runner = Arc::new(Observed::default());
    let store = Memory::default();
    let service = Schedules::spawn(Box::new(store.clone()), runner.clone()).unwrap();
    let mut params = input();
    params["runOnCreate"] = json!(false);
    let id = service
        .execute("schedule.create.request", params)
        .await
        .unwrap()["schedule"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let first = service
        .execute("schedule.run_once.request", json!({"scheduleId":id}))
        .await
        .unwrap();
    let second = service
        .execute("schedule.run_once.request", json!({"scheduleId":id}))
        .await
        .unwrap();
    service.shutdown().await.unwrap();

    let runs: Vec<String> = [first, second]
        .iter()
        .map(|result| {
            let runs = result["schedule"]["runs"].as_array().unwrap();
            runs.last().unwrap()["id"].as_str().unwrap().to_owned()
        })
        .collect();
    let settled = runner.settled.lock().unwrap().clone();
    assert_eq!(
        settled,
        runs.iter()
            .map(|run| (id.clone(), run.clone(), RunStatus::Succeeded, None))
            .collect::<Vec<_>>()
    );
    let saved = &store.0.lock().unwrap().0[0];
    assert!(
        saved
            .runs
            .iter()
            .all(|run| run.status == RunStatus::Succeeded)
    );
}

#[tokio::test]
async fn runs_that_ended_recently_are_reported_again_whenever_the_worker_opens() {
    let store = Memory::default();
    let service = Schedules::spawn(Box::new(store.clone()), Arc::new(Observed::default())).unwrap();
    let mut params = input();
    params["runOnCreate"] = json!(false);
    service
        .execute("schedule.create.request", params)
        .await
        .unwrap();
    service.shutdown().await.unwrap();
    let run = |id: &str, status: RunStatus, ended: Option<chrono::DateTime<Utc>>| Run {
        id: id.into(),
        scheduled_for: Utc::now(),
        started_at: Utc::now(),
        ended_at: ended,
        status,
        agent_id: None,
        workspace_id: None,
        output: None,
        error: None,
    };
    let interrupted = run(
        "22222222-2222-4222-8222-222222222222",
        RunStatus::Running,
        None,
    );
    // Settled while the daemon stopped, so its first report may never have been delivered.
    let recent = run(
        "33333333-3333-4333-8333-333333333333",
        RunStatus::Succeeded,
        Some(Utc::now() - chrono::TimeDelta::minutes(1)),
    );
    let old = run(
        "44444444-4444-4444-8444-444444444444",
        RunStatus::Failed,
        Some(Utc::now() - chrono::TimeDelta::hours(2)),
    );
    let id = {
        let mut state = store.0.lock().unwrap();
        state.0[0]
            .runs
            .extend([interrupted.clone(), recent.clone(), old]);
        state.0[0].id.clone()
    };

    let runner = Arc::new(Observed::default());
    let reopened = Schedules::spawn(Box::new(store.clone()), runner.clone()).unwrap();
    reopened.shutdown().await.unwrap();
    let again = Arc::new(Observed::default());
    let third = Schedules::spawn(Box::new(store.clone()), again.clone()).unwrap();
    third.shutdown().await.unwrap();

    for observed in [&runner, &again] {
        let mut settled = observed.settled.lock().unwrap().clone();
        settled.sort_by(|left, right| left.1.cmp(&right.1));
        assert_eq!(settled.len(), 2, "{settled:?}");
        assert_eq!(
            (&settled[0].0, &settled[0].1, settled[0].2),
            (&id, &interrupted.id, RunStatus::Failed)
        );
        assert!(settled[0].3.as_deref().unwrap().contains("restarted"));
        assert_eq!(
            (&settled[1].0, &settled[1].1, settled[1].2),
            (&id, &recent.id, RunStatus::Succeeded)
        );
    }
}
