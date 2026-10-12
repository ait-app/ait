//! Host validation leaves the actor free to persist running occurrences and detects conflicting edits.
use super::*;
use domain::schedule::Target;

#[derive(Debug, Default)]
struct ValidatingRunner {
    validating: tokio::sync::Notify,
    validated: tokio::sync::Notify,
    running: tokio::sync::Notify,
    finished: tokio::sync::Notify,
}
impl Runner for ValidatingRunner {
    fn resolve(
        &self,
        workspace_id: String,
        mut target: Target,
    ) -> Pin<Box<dyn Future<Output = Result<Target, Error>> + Send + '_>> {
        Box::pin(async move {
            if workspace_id == "missing" {
                return Err(Error::Invalid);
            }
            if let Target::NewAgent { config } = &mut target {
                if config["provider"] == "slow" {
                    self.validating.notify_one();
                    self.validated.notified().await;
                }
                config["cwd"] = json!("/canonical/workspace");
            }
            Ok(target)
        })
    }
    fn run(
        &self,
        _: Schedule,
        _: String,
        progress: Progress,
        _: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Outcome> + Send + '_>> {
        Box::pin(async move {
            self.running.notify_one();
            self.finished.notified().await;
            progress
                .record(Some("agent".into()), Some("wks-test".into()))
                .await
                .unwrap();
            Outcome {
                output: Some("persisted while validating".into()),
                ..Outcome::default()
            }
        })
    }
}
fn slow_target() -> Value {
    json!({"type":"new-agent","config":{"provider":"slow","cwd":"/untrusted/path"}})
}

#[tokio::test]
async fn validation_canonicalizes_placement_and_rejects_missing_owner_without_writing() {
    let store = Memory::default();
    let service = Schedules::spawn(
        Box::new(store.clone()),
        Arc::new(ValidatingRunner::default()),
    )
    .unwrap();
    let mut params = input();
    params["runOnCreate"] = json!(false);
    params["workspaceId"] = json!("missing");
    assert_eq!(
        service
            .execute("schedule.create.request", params.clone())
            .await
            .unwrap_err(),
        Error::Invalid
    );
    assert!(store.0.lock().unwrap().0.is_empty());
    params["workspaceId"] = json!("wks-test");
    params["target"] = json!({"type":"new-agent","config":{"provider":"codex"}});
    let response = service
        .execute("schedule.create.request", params)
        .await
        .unwrap();
    assert_eq!(
        response["schedule"]["target"]["config"]["cwd"],
        "/canonical/workspace"
    );
    service.shutdown().await.unwrap();
}

#[tokio::test]
async fn slow_validation_does_not_block_queries_checkpoints_or_settlement() {
    let runner = Arc::new(ValidatingRunner::default());
    let service = Schedules::spawn(Box::new(Memory::default()), runner.clone()).unwrap();
    let mut params = input();
    params["runOnCreate"] = json!(false);
    let id = service
        .execute("schedule.create.request", params.clone())
        .await
        .unwrap()["schedule"]["id"]
        .clone();
    let client = service.clone();
    let running = tokio::spawn(async move {
        client
            .execute("schedule.run_once.request", json!({"scheduleId":id}))
            .await
    });
    runner.running.notified().await;
    params["target"] = slow_target();
    let client = service.clone();
    let validating =
        tokio::spawn(async move { client.execute("schedule.create.request", params).await });
    runner.validating.notified().await;
    tokio::time::timeout(
        Duration::from_secs(1),
        service.execute("schedule.list.request", json!({})),
    )
    .await
    .unwrap()
    .unwrap();
    runner.finished.notify_one();
    let result = tokio::time::timeout(Duration::from_secs(1), running)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        result["schedule"]["runs"][0]["output"],
        "persisted while validating"
    );
    assert!(!validating.is_finished());
    runner.validated.notify_one();
    assert!(validating.await.unwrap().is_ok());
    service.shutdown().await.unwrap();
}

#[tokio::test]
async fn validated_target_update_cannot_overwrite_a_newer_revision() {
    let runner = Arc::new(ValidatingRunner::default());
    let service = Schedules::spawn(Box::new(Memory::default()), runner.clone()).unwrap();
    let mut params = input();
    params["runOnCreate"] = json!(false);
    let id = service
        .execute("schedule.create.request", params)
        .await
        .unwrap()["schedule"]["id"]
        .clone();
    let client = service.clone();
    let key = id.clone();
    let editing = tokio::spawn(async move {
        client
            .execute(
                "schedule.update.request",
                json!({"scheduleId":key,"target":slow_target()}),
            )
            .await
    });
    runner.validating.notified().await;
    service
        .execute(
            "schedule.update.request",
            json!({"scheduleId":id,"name":"newer name"}),
        )
        .await
        .unwrap();
    runner.validated.notify_one();
    assert_eq!(editing.await.unwrap().unwrap_err(), Error::Conflict);
    let record = service
        .execute("schedule.inspect.request", json!({"scheduleId":id}))
        .await
        .unwrap();
    assert_eq!(record["schedule"]["name"], "newer name");
    assert_eq!(record["schedule"]["target"]["type"], "agent");
    service.shutdown().await.unwrap();
}
