use std::time::Duration;

use domain::storage::daemon_config::{DaemonConfigReload, DaemonConfigStoreError};
use metadata::daemon::service::{Daemon, DaemonRuntime};
use model::storage::daemon_config::DaemonConfigStore;
use serde_json::json;

use super::*;

/// A daemon configuration store that returns `config`, or fails when it is `None`.
#[derive(Debug)]
struct Store {
    config: Option<Value>,
}

impl DaemonConfigStore for Store {
    fn get(&self) -> Result<Value, DaemonConfigStoreError> {
        self.config.clone().ok_or(DaemonConfigStoreError::Io)
    }

    fn patch(&self, _patch: &Value) -> Result<Value, DaemonConfigStoreError> {
        Err(DaemonConfigStoreError::Io)
    }

    fn reload(&self) -> Result<DaemonConfigReload, DaemonConfigStoreError> {
        Err(DaemonConfigStoreError::Io)
    }
}

fn daemon(config: Option<Value>) -> Daemon {
    Daemon::new(
        DaemonRuntime {
            server_id: "srv-1".to_owned(),
            version: None,
            pid: 42,
            executable: "/bin/server".to_owned(),
            started_at: None,
            listen: "127.0.0.1:6767".to_owned(),
        },
        Box::new(Store { config }),
    )
}

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
    let acceptance = PeerAcceptance::follow(&events, || false).expect("observer");
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
    let accepting = daemon(Some(json!({"mcp":{"enabled":true,"acceptPeers":true}})));
    let events = SessionEvents::default();
    let acceptance = PeerAcceptance::follow(&events, || read(Some(&accepting))).expect("observer");
    assert!(*acceptance.receiver().borrow());
}

#[test]
fn startup_defaults_off_without_a_daemon_or_a_readable_configuration() {
    assert!(!read(None), "no daemon service installed");
    assert!(!read(Some(&daemon(None))), "get_config fails");
    assert!(
        !read(Some(&daemon(Some(json!({"mcp":{"enabled":true}}))))),
        "a configuration without the key"
    );
    let events = SessionEvents::default();
    let acceptance =
        PeerAcceptance::follow(&events, || read(Some(&daemon(None)))).expect("observer");
    assert!(!*acceptance.receiver().borrow());
}

async fn changed(accepting: &mut watch::Receiver<bool>) {
    tokio::time::timeout(Duration::from_secs(5), accepting.changed())
        .await
        .expect("a change within the guard")
        .expect("sender alive");
}
