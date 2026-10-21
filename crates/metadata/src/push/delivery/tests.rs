use std::collections::VecDeque;
use std::sync::Mutex;

use domain::agent_runtime::{PersistedAgentRuntimeRecord, registry::AgentRuntimeRegistryError};
use model::storage::push::TokenStore;
use model::{Lifecycle, Limits, Runtime, ServerInfo};

use super::*;
use crate::push::ports::DeliveryFuture;
use crate::push::service::PushTokens;

#[derive(Debug, Default)]
struct Registry(Mutex<Option<PersistedAgentRuntimeRecord>>);

impl AgentRuntimeRegistry for Registry {
    fn initialize(&self) -> Result<(), AgentRuntimeRegistryError> {
        Ok(())
    }
    fn list(&self) -> Result<Vec<PersistedAgentRuntimeRecord>, AgentRuntimeRegistryError> {
        Ok(self.0.lock().unwrap().clone().into_iter().collect())
    }
    fn get(
        &self,
        id: &str,
    ) -> Result<Option<PersistedAgentRuntimeRecord>, AgentRuntimeRegistryError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .clone()
            .filter(|record| record.id == id))
    }
    fn upsert(
        &self,
        record: &PersistedAgentRuntimeRecord,
    ) -> Result<(), AgentRuntimeRegistryError> {
        *self.0.lock().unwrap() = Some(record.clone());
        Ok(())
    }
    fn update(
        &self,
        _: &str,
        _: &dyn Fn(&PersistedAgentRuntimeRecord) -> PersistedAgentRuntimeRecord,
    ) -> Result<Option<PersistedAgentRuntimeRecord>, AgentRuntimeRegistryError> {
        unreachable!()
    }
    fn remove(&self, _: &str) -> Result<bool, AgentRuntimeRegistryError> {
        Ok(self.0.lock().unwrap().take().is_some())
    }
}

#[derive(Debug, Default)]
struct Store(Mutex<Value>);
impl TokenStore for Store {
    fn load(&self) -> Result<Value, domain::storage::push::PushError> {
        Ok(json!({}))
    }
    fn save(&self, value: &Value) -> Result<(), domain::storage::push::PushError> {
        *self.0.lock().unwrap() = value.clone();
        Ok(())
    }
}

#[derive(Debug, Default)]
struct Transport {
    sent: Mutex<Vec<Notification>>,
    results: Mutex<VecDeque<Result<(), DeliveryError>>>,
    changed: tokio::sync::Notify,
}
impl PushTransport for Transport {
    fn send<'a>(&'a self, notification: &'a Notification, _: &'a str) -> DeliveryFuture<'a> {
        Box::pin(async move {
            self.sent.lock().unwrap().push(notification.clone());
            self.changed.notify_one();
            self.results.lock().unwrap().pop_front().unwrap_or(Ok(()))
        })
    }
}

fn fixture() -> (Arc<State>, Arc<Transport>, Arc<Registry>) {
    let transport = Arc::new(Transport::default());
    let registry = Arc::new(Registry::default());
    registry
        .upsert(
            &serde_json::from_value(json!({
                "id":"agent","provider":"test","cwd":"/project","createdAt":"now","updatedAt":"now",
                "workspaceId":"workspace","title":"Build completed","requiresAttention":true,
                "attentionReason":"finished","attentionTimestamp":"now",
            }))
            .unwrap(),
        )
        .unwrap();
    let mut tokens = PushTokens::open(Box::<Store>::default(), 0)
        .unwrap()
        .with_delivery(transport.clone(), registry.clone());
    tokens
        .renew("device", chrono::Utc::now().timestamp_millis())
        .unwrap();
    let state = Arc::new(State {
        runtime: Arc::new(Runtime::new(ServerInfo {
            server_id: "host".to_owned(),
            version: None,
            instance_id: "instance".to_owned(),
            listen: "127.0.0.1:1".to_owned(),
            lifecycle: Lifecycle::Ready,
            protocol: model::VERSION,
            capabilities: vec![],
            implemented_capabilities: vec![],
            features: vec![],
            limits: Limits::default(),
        })),
        push_tokens: Some(Arc::new(Mutex::new(tokens))),
        daemon: None,
        directory: None,
        workspace_labels: None,
        workspace_automation: None,
        workspace_state: None,
        worktrees: None,
        workspace_recovery: None,
        github_projects: None,
        creations: model::creation::Creations::default(),
        session_events: model::session::SessionEvents::default(),
        has_agent_execution: true,
        has_terminals: true,
        has_git_fetch: false,
    });
    (state, transport, registry)
}

#[tokio::test]
async fn committed_attention_pushes_after_disconnect_and_worker_drains_on_shutdown() {
    let (state, transport, _) = fixture();
    let _worker = start(&state);
    assert!(start(&state).is_none()); // Installation is idempotent.
    let connection = state.session_events.connect();
    drop(connection);
    state.session_events.publish(
        SessionEventKind::AgentAttention,
        &json!({"agentId":"agent","reason":"finished","timestamp":"now"}),
    );
    tokio::time::timeout(Duration::from_secs(2), transport.changed.notified())
        .await
        .unwrap();
    state.start_draining();
    tokio::time::timeout(Duration::from_secs(2), state.tasks.wait())
        .await
        .unwrap();
    let sent = transport.sent.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].title, "Agent finished");
    assert_eq!(
        sent[0].data,
        json!({"serverId":"host","workspaceId":"workspace","agentId":"agent","reason":"finished"})
    );
}

#[tokio::test]
async fn foreground_devices_are_local_only_and_unregistered_devices_are_removed() {
    let (state, transport, _) = fixture();
    let connection = state.session_events.connect();
    connection.set_push_token(Some("device".to_owned()));
    connection.heartbeat(serde_json::from_value(json!({"deviceType":"mobile","focusedAgentId":null,"appVisible":true,"lastActivityAt":chrono::Utc::now().to_rfc3339()})).unwrap()).unwrap();
    let dependencies = state
        .push_tokens
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .delivery
        .take()
        .unwrap();
    let payload = json!({"agentId":"agent","reason":"finished","timestamp":"now"});
    deliver(
        &state,
        &dependencies,
        SessionEventKind::AgentAttention,
        payload.clone(),
    )
    .await;
    assert!(transport.sent.lock().unwrap().is_empty());
    drop(connection);
    transport
        .results
        .lock()
        .unwrap()
        .push_back(Err(DeliveryError::Unregistered));
    deliver(
        &state,
        &dependencies,
        SessionEventKind::AgentAttention,
        payload,
    )
    .await;
    let mut tokens = state.push_tokens.as_ref().unwrap().lock().unwrap();
    assert!(tokens.active(0).is_empty());
    tokens.renew("device", 0).unwrap();
    assert!(
        tokens.active(0).is_empty(),
        "heartbeats cannot resurrect an invalid token"
    );
    tokens.register("device", 0).unwrap();
    assert_eq!(tokens.active(0), ["device"]);
}

#[test]
fn content_rejects_stale_deleted_archived_internal_and_unknown_attention() {
    let (_, _, registry) = fixture();
    let payload = json!({"agentId":"agent","reason":"permission","timestamp":"now"});
    assert_eq!(
        notification(
            registry.as_ref(),
            SessionEventKind::AgentAttention,
            &payload,
            "host"
        )
        .unwrap()
        .unwrap()
        .title,
        "Agent needs permission"
    );
    for field in ["internal", "archived", "stale", "cleared", "deleted"] {
        let (_, _, registry) = fixture();
        {
            let mut stored = registry.0.lock().unwrap();
            match field {
                "internal" => stored.as_mut().unwrap().internal = true,
                "archived" => stored.as_mut().unwrap().archived_at = Some("now".to_owned()),
                "stale" => stored.as_mut().unwrap().attention_timestamp = Some("later".to_owned()),
                "cleared" => stored.as_mut().unwrap().requires_attention = false,
                "deleted" => *stored = None,
                _ => unreachable!(),
            }
        }
        assert!(
            notification(
                registry.as_ref(),
                SessionEventKind::AgentAttention,
                &payload,
                "host"
            )
            .unwrap()
            .is_none()
        );
    }
    let terminal = notification(registry.as_ref(), SessionEventKind::TerminalAttention, &json!({"terminalId":"terminal","workspaceId":"workspace","title":"Done","body":"Completed"}), "host").unwrap().unwrap();
    assert_eq!(terminal.data["terminalId"], "terminal");
    assert!(
        notification(
            registry.as_ref(),
            SessionEventKind::ServerInfo,
            &payload,
            "host"
        )
        .unwrap()
        .is_none()
    );
}

#[tokio::test]
async fn rejected_deliveries_are_not_retried() {
    let transport = Transport::default();
    transport
        .results
        .lock()
        .unwrap()
        .push_back(Err(DeliveryError::Rejected));
    let notification = Notification {
        title: "Done".to_owned(),
        body: String::new(),
        data: json!({}),
    };
    assert_eq!(
        send_with_retry(&transport, &notification, "device").await,
        Err(DeliveryError::Rejected)
    );
    assert_eq!(transport.sent.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn temporary_delivery_failure_is_retried() {
    let transport = Transport::default();
    transport
        .results
        .lock()
        .unwrap()
        .extend([Err(DeliveryError::Retryable(0)), Ok(())]);
    let notification = Notification {
        title: "Done".to_owned(),
        body: String::new(),
        data: json!({}),
    };
    assert_eq!(
        send_with_retry(&transport, &notification, "device").await,
        Ok(())
    );
    assert_eq!(transport.sent.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn dropping_worker_cancels_without_requiring_server_shutdown() {
    let (state, _, _) = fixture();
    let worker = start(&state).unwrap();
    drop(worker);
    state.tasks.close();
    tokio::time::timeout(Duration::from_secs(2), state.tasks.wait())
        .await
        .unwrap();
}

#[tokio::test]
async fn connection_registration_persists_before_ownership_and_invalidated_owners_cannot_renew() {
    let (state, _, _) = fixture();
    let mut connection = crate::connection::Connection::new(state.session_events.connect());
    crate::push::connection::register(
        json!({"token":"registered-device"}),
        &state,
        &mut connection,
    )
    .await
    .unwrap();
    assert!(
        connection
            .session
            .as_ref()
            .unwrap()
            .owns_push_token("registered-device")
    );
    state
        .push_tokens
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .revoke("registered-device")
        .unwrap();
    state.session_events.clear_push_token("registered-device");
    crate::push::connection::heartbeat(&state, &connection)
        .await
        .unwrap();
    assert!(
        !state
            .push_tokens
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .active(0)
            .contains(&"registered-device".to_owned())
    );
    crate::push::connection::register(
        json!({"token":"registered-device"}),
        &state,
        &mut connection,
    )
    .await
    .unwrap();
    assert!(
        connection
            .session
            .as_ref()
            .unwrap()
            .owns_push_token("registered-device")
    );
}
