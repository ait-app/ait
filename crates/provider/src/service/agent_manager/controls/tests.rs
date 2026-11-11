use super::*;
use std::sync::{Arc, Mutex};

use domain::agent_runtime::registry::AgentRuntimeRegistry;

use crate::ports::agent_session::AgentSession;
use crate::service::agent_manager::{LiveAgent, tests::MemoryRegistry};

fn child(id: &str, parent: &str) -> NativeSubagent {
    NativeSubagent {
        persistence: None,
        id: id.to_owned(),
        parent_id: parent.to_owned(),
        cwd: "/tmp".to_owned(),
        descriptor: json!({}),
    }
}

#[test]
fn descendants_isolates_roots_follows_nested_children_and_rejects_cycles() {
    let result = descendants(
        "root",
        vec![
            child("grandchild", "child"),
            child("foreign", "other"),
            child("child", "root"),
        ],
    )
    .unwrap();
    assert_eq!(
        result
            .iter()
            .map(|child| child.id.as_str())
            .collect::<Vec<_>>(),
        vec!["child", "grandchild"]
    );
    assert!(descendants("root", vec![child("child", "root"), child("root", "child")]).is_err());
}

#[derive(Debug)]
struct PermissionSession {
    pending: Option<Value>,
    answers: Arc<Mutex<Vec<Value>>>,
}

#[async_trait::async_trait]
impl AgentSession for PermissionSession {
    fn provider(&self) -> &'static str {
        "test"
    }

    async fn runtime_info(
        &mut self,
    ) -> Result<domain::agent_runtime::StoredAgentRuntimeInfo, AgentSessionError> {
        Err(AgentSessionError::Unavailable)
    }

    fn persistence(&self) -> Option<AgentPersistenceHandle> {
        None
    }

    fn pending_permissions(&self) -> Vec<Value> {
        self.pending.iter().cloned().collect()
    }

    async fn respond_permission(
        &mut self,
        id: &str,
        response: &Value,
    ) -> Result<(), AgentSessionError> {
        if self
            .pending
            .as_ref()
            .is_none_or(|pending| pending["id"] != id)
        {
            return Err(AgentSessionError::Rejected);
        }
        self.answers.lock().unwrap().push(response.clone());
        self.pending = None;
        Ok(())
    }

    async fn close(&mut self) -> Result<(), AgentSessionError> {
        Ok(())
    }
}

#[tokio::test]
async fn permission_returns_safe_resolution_and_preserves_native_answer_and_live_question() {
    for (secret, behavior, capable) in [
        (true, "allow", true),
        (true, "deny", false),
        (true, "deny", true),
        (false, "allow", false),
    ] {
        let registry = MemoryRegistry::default();
        let record: PersistedAgentRuntimeRecord = serde_json::from_value(json!({
            "id":"agent", "provider":"test", "cwd":"/tmp",
            "createdAt":"now", "updatedAt":"now",
            "requiresAttention":true, "attentionReason":"permission"
        }))
        .unwrap();
        registry.upsert(&record).unwrap();
        let mut manager = AgentManager::new(Box::new(registry.clone()));
        let answers = Arc::new(Mutex::new(Vec::new()));
        let request = json!({
            "id":"permission", "kind":"question",
            "input":{"questions":[{"id":"value","secret":secret,"question":"Live prompt"}]}
        });
        manager.live.insert(
            record.id.clone(),
            LiveAgent::new(
                Box::new(PermissionSession {
                    pending: Some(request.clone()),
                    answers: answers.clone(),
                }),
                Arc::new(tokio::sync::Semaphore::new(1))
                    .try_acquire_owned()
                    .unwrap(),
                record.clone(),
                true,
            ),
        );
        let connection = manager.events().connect();
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = events.clone();
        let subscription = connection
            .subscribe(
                domain::session::protocol::EventsRequest {
                    events: vec!["agent_permission_resolved".into()],
                    notifications: false,
                },
                Arc::new(move |_, value| {
                    captured.lock().unwrap().push(value);
                    Ok(())
                }),
            )
            .unwrap();
        subscription.activate().unwrap();
        let mut snapshot = json!({});
        manager.control_snapshot(&record, &mut snapshot);
        assert_eq!(snapshot["pendingPermissions"], json!([request]));

        let response = json!({
            "behavior":behavior,
            "updatedInput":{"answers":{"value":"  private answer  "}}
        });
        if secret && behavior == "allow" {
            assert_eq!(
                manager
                    .permission(
                        "agent",
                        "permission",
                        &response,
                        PermissionContext::default()
                    )
                    .await,
                Err(ErrorCode::UnsupportedCapability)
            );
            assert!(answers.lock().unwrap().is_empty());
            assert!(events.lock().unwrap().is_empty());
            assert!(registry.get("agent").unwrap().unwrap().requires_attention);
            manager.control_snapshot(&record, &mut snapshot);
            assert_eq!(snapshot["pendingPermissions"], json!([request]));
        }
        let context = PermissionContext::negotiated(capable);
        let resolution = manager
            .permission("agent", "permission", &response, context)
            .await
            .unwrap();
        let expected = if secret {
            json!({"behavior":behavior,"redacted":true})
        } else {
            response.clone()
        };
        assert_eq!(resolution, expected);
        assert_eq!(events.lock().unwrap()[0]["resolution"], expected);
        assert_eq!(*answers.lock().unwrap(), vec![response.clone()]);
        assert!(!registry.get("agent").unwrap().unwrap().requires_attention);
        assert_eq!(
            manager
                .permission("agent", "permission", &response, context)
                .await,
            Err(ErrorCode::InvalidMessage)
        );
        assert_eq!(*answers.lock().unwrap(), vec![response]);
        manager.close_all().await.unwrap();
    }
}
