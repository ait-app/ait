use std::sync::Mutex;

use domain::session::protocol::SessionEventKind;

use super::*;

#[derive(Debug, Default)]
struct Publisher(Mutex<Vec<String>>);

impl model::hub::EventPublisher for Publisher {
    fn publish(&self, method: &str, _: &Value) -> bool {
        self.0.lock().unwrap().push(method.to_owned());
        false
    }
}

#[tokio::test]
async fn daemon_publisher_attaches_to_shared_sources_without_client_connections() {
    let fixture = Fixture::start().await;
    assert!(fixture.api.hub_center().state().borrow().scope.is_none());
    let publisher = Arc::new(Publisher::default());
    fixture.api.set_event_publisher(publisher.clone());
    fixture
        .api
        .shared
        .metadata
        .session_events
        .publish(SessionEventKind::ServerInfo, &json!({"ready":true}));
    fixture
        .api
        .shared
        .metadata
        .creations
        .begin(
            domain::creation::protocol::Kind::Agent,
            "publication-test",
            json!({}),
        )
        .unwrap();
    assert_eq!(
        publisher.0.lock().unwrap().as_slice(),
        &["status.server_info", "agent.create.update"]
    );
    fixture.stop().await;
}
