use std::time::Duration;

use serde_json::json;

use super::*;

#[test]
fn accepting_requires_both_switches() {
    for (config, expected) in [
        (json!({"mcp":{"enabled":true,"acceptPeers":true}}), true),
        (json!({"mcp":{"acceptPeers":true}}), true),
        (json!({"mcp":{"enabled":false,"acceptPeers":true}}), false),
        (json!({"mcp":{"enabled":true,"acceptPeers":false}}), false),
        (json!({"mcp":{"enabled":true}}), false),
        (json!({}), false),
    ] {
        assert_eq!(accepting(&config), expected, "{config}");
    }
}

#[tokio::test]
async fn follows_committed_configuration_events() {
    let events = SessionEvents::default();
    let acceptance = PeerAcceptance::follow(&events, false).expect("observer");
    let mut accepting = acceptance.receiver();
    assert!(!*accepting.borrow_and_update());

    events.publish(
        SessionEventKind::ServerInfo,
        &json!({"config":{"mcp":{"acceptPeers":true}}}),
    );
    events.publish(
        SessionEventKind::DaemonConfig,
        &json!({"status":"daemon_config_changed","config":{"mcp":{"acceptPeers":true}}}),
    );
    changed(&mut accepting).await;
    assert!(
        *accepting.borrow_and_update(),
        "only the configuration event counts"
    );

    events.publish(
        SessionEventKind::DaemonConfig,
        &json!({"config":{"mcp":{"enabled":true,"acceptPeers":true}}}),
    );
    assert!(
        !accepting.has_changed().expect("sender alive"),
        "an unchanged value does not wake observers"
    );
    events.publish(
        SessionEventKind::DaemonConfig,
        &json!({"config":{"mcp":{"enabled":false,"acceptPeers":true}}}),
    );
    changed(&mut accepting).await;
    assert!(!*accepting.borrow_and_update());
}

#[test]
fn starts_from_the_value_read_at_startup() {
    let events = SessionEvents::default();
    let acceptance = PeerAcceptance::follow(&events, true).expect("observer");
    assert!(*acceptance.receiver().borrow());
}

async fn changed(accepting: &mut watch::Receiver<bool>) {
    tokio::time::timeout(Duration::from_secs(5), accepting.changed())
        .await
        .expect("a change within the guard")
        .expect("sender alive");
}
