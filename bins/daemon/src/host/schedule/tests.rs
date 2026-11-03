use super::available_agent;
use serde_json::json;

#[test]
fn observed_agents_cannot_be_scheduled_or_reused_as_existing_targets() {
    assert!(available_agent(
        &json!({"agent":{"management":"managed","archivedAt":null}})
    ));
    assert!(!available_agent(
        &json!({"agent":{"management":"observed","archivedAt":null}})
    ));
    assert!(!available_agent(
        &json!({"agent":{"archivedAt":null,"capabilities":{"supportsMessageSend":false}}})
    ));
    assert!(!available_agent(
        &json!({"agent":{"archivedAt":"2026-10-31T00:00:00Z"}})
    ));
    assert!(!available_agent(&json!({"agent":null})));
}
