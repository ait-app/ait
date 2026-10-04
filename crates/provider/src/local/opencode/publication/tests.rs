use super::*;
use crate::local::opencode::{
    Driver,
    tests::{fixture::Fixture, invocation},
};
use async_trait::async_trait;
use std::sync::Mutex;

#[derive(Default)]
struct Collected(Mutex<Vec<ProgressEvent>>);

#[async_trait]
impl ProgressSink for Collected {
    async fn report(&self, event: ProgressEvent) {
        self.0.lock().unwrap().push(event);
    }
}

fn delta(version: Version) -> ProgressEvent {
    ProgressEvent::TextDelta {
        id: match version {
            Version::V1 => "prt_0123456789abABCDEFGHIJKLM1",
            Version::V2 => "answer1",
        }
        .into(),
        delta: "ans".into(),
    }
}

#[tokio::test]
async fn delayed_history_defers_the_turn_without_replaying_input_or_partial_text() {
    for version in [Version::V1, Version::V2] {
        let fixture = Fixture::start(version).await;
        let mut connection = Driver::new(fixture.binary.clone())
            .open(invocation(fixture.cwd.clone()))
            .await
            .unwrap();
        let _events = connection.submit().await.unwrap();
        let answer = fixture.state.lock().unwrap().history.pop().unwrap();
        let collected = Arc::new(Collected::default());
        let sink: Arc<dyn ProgressSink> = collected.clone();
        let mut publication = Publication::default();
        publication
            .report(&connection, &sink, delta(version))
            .await
            .unwrap();
        fixture.state.lock().unwrap().history.push(answer);
        publication
            .report(&connection, &sink, delta(version))
            .await
            .unwrap();
        assert!(collected.0.lock().unwrap().is_empty());
        let snapshot = super::super::session::snapshot(
            &connection.runtime.api,
            &connection.prepared.id,
            &connection.invocation,
        )
        .await
        .unwrap();
        let entries = projection::records(&snapshot.messages, &BTreeMap::new()).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].item["type"], "user_message");
        assert_eq!(entries[1].item["text"], "answer");
        assert_eq!(fixture.state.lock().unwrap().submissions, 1);
    }
}
