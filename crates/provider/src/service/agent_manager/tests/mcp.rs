use super::*;
use crate::service::agent_mcp::{
    AgentMcp,
    tests::{Policy, TOKEN},
};

#[derive(Debug)]
pub(super) enum Support {
    Unsupported,
    Http,
}

fn connection(policy: Arc<Policy>, port: u16) -> AgentMcp {
    AgentMcp::new(
        format!("127.0.0.1:{port}").parse().unwrap(),
        TOKEN.into(),
        policy,
    )
}

#[tokio::test]
async fn mcp_is_private_and_preserved_for_subsequent_inputs() {
    let (manager, registry, client) = make_manager();
    client.0.lock().unwrap().mcp_support = Support::Http;
    let policy = Arc::new(Policy(Mutex::new(json!({}))));
    let mut manager = manager
        .with_agent_mcp(connection(policy.clone(), 7316))
        .fork(None);
    let created = manager
        .create("agent-a", &spec(), AgentRegistration::default())
        .await
        .unwrap();
    let launched = client.0.lock().unwrap().create_specs[0].config.clone();
    assert_eq!(
        launched.mcp_servers.as_ref().unwrap()["ait"]["headers"]["X-Ait-Agent-Id"],
        "agent-a"
    );
    assert!(created.config.as_ref().unwrap().mcp_servers.is_none());
    *policy.0.lock().unwrap() = json!({"mcp":{"injectIntoAgents":false}});
    for text in ["first", "second"] {
        manager.send("agent-a", text).await.unwrap();
        client
            .0
            .lock()
            .unwrap()
            .events
            .push_back(AgentTurnEvent::Completed(Some("done".into())));
        manager.poll().await.unwrap();
    }
    assert_eq!(
        client.0.lock().unwrap().start_configs,
        vec![launched.clone(), launched]
    );
    assert!(
        !serde_json::to_string(&registry.list().unwrap())
            .unwrap()
            .contains(TOKEN)
    );
    assert!(
        !serde_json::to_string(&manager.live["agent-a"].record)
            .unwrap()
            .contains(TOKEN)
    );
}

#[tokio::test]
async fn resume_restore_and_import_rebind_without_persisting_launch_settings() {
    let policy = Arc::new(Policy(Mutex::new(json!({}))));
    let (manager, registry, client) = make_manager();
    client.0.lock().unwrap().mcp_support = Support::Http;
    let mut manager = manager.with_agent_mcp(connection(policy.clone(), 7316));
    let record = stored_record();
    registry.upsert(&record).unwrap();
    manager.resume("agent-1").await.unwrap();
    manager.close("agent-1").await.unwrap();
    manager.mcp = Some(connection(policy.clone(), 8321));
    manager
        .restore("agent-1", &crate::protocol::resume::Overrides::default())
        .await
        .unwrap();
    let mut imported = stored_record();
    imported.id = "imported".into();
    manager.restore_new(imported).await.unwrap();
    let state = client.0.lock().unwrap();
    let urls: Vec<_> = state
        .resume_specs
        .iter()
        .map(|s| s.config.mcp_servers.as_ref().unwrap()["ait"]["url"].clone())
        .collect();
    assert_eq!(
        urls,
        vec![
            json!("http://127.0.0.1:7316/mcp/agents"),
            json!("http://127.0.0.1:8321/mcp/agents"),
            json!("http://127.0.0.1:8321/mcp/agents")
        ]
    );
    assert_eq!(
        state.resume_specs[2].config.mcp_servers.as_ref().unwrap()["ait"]["headers"]["X-Ait-Agent-Id"],
        "imported"
    );
    drop(state);
    assert!(
        registry
            .list()
            .unwrap()
            .iter()
            .all(|r| r.config.as_ref().unwrap().mcp_servers.is_none())
    );
    assert!(
        !serde_json::to_string(&registry.list().unwrap())
            .unwrap()
            .contains(TOKEN)
    );
}

#[tokio::test]
async fn unsupported_disabled_and_history_sessions_are_not_injected() {
    for (supported, flags, archived) in [
        (false, json!({}), false),
        (true, json!({"mcp":{"enabled":false}}), false),
        (true, json!({"mcp":{"injectIntoAgents":false}}), false),
        (true, json!({}), true),
    ] {
        let (manager, registry, client) = make_manager();
        client.0.lock().unwrap().mcp_support = if supported {
            Support::Http
        } else {
            Support::Unsupported
        };
        let mut manager =
            manager.with_agent_mcp(connection(Arc::new(Policy(Mutex::new(flags))), 7316));
        let mut record = stored_record();
        if archived {
            record.archived_at = Some("2026-10-29T00:00:00Z".into());
        }
        registry.upsert(&record).unwrap();
        manager.resume("agent-1").await.unwrap();
        assert!(
            client.0.lock().unwrap().resume_specs[0]
                .config
                .mcp_servers
                .is_none()
        );
    }
}

#[tokio::test]
async fn guest_created_agents_never_receive_the_daemon_mcp() {
    let (manager, registry, client) = make_manager();
    client.0.lock().unwrap().mcp_support = Support::Http;
    let policy = Arc::new(Policy(Mutex::new(json!({}))));
    let mut manager = manager.with_agent_mcp(connection(policy, 7316)).fork(None);
    let labels = BTreeMap::from([(
        crate::service::agent_mcp::GUEST_SHARE_LABEL.to_owned(),
        "share".to_owned(),
    )]);
    manager
        .create(
            "agent-guest",
            &spec(),
            AgentRegistration {
                labels: labels.clone(),
                ..AgentRegistration::default()
            },
        )
        .await
        .unwrap();
    assert!(
        client.0.lock().unwrap().create_specs[0]
            .config
            .mcp_servers
            .is_none()
    );
    let mut record = stored_record();
    record.labels = labels;
    registry.upsert(&record).unwrap();
    manager.resume("agent-1").await.unwrap();
    assert!(
        client.0.lock().unwrap().resume_specs[0]
            .config
            .mcp_servers
            .is_none()
    );
}
