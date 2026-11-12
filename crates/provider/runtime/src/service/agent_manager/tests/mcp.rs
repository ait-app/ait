use super::*;
use crate::service::agent_mcp::{
    AgentMcp,
    tests::{FakeScopes, Grant, Policy, TOKEN},
};

#[derive(Debug)]
pub(super) enum Support {
    Unsupported,
    Http,
    /// HTTP MCP, with tool pre-approvals that only add to the session's own.
    AdditiveHttp,
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

fn scoped(
    grant: Grant,
    flags: Value,
) -> (AgentManager, MemoryRegistry, FakeClient, Arc<FakeScopes>) {
    let (manager, registry, client) = make_manager();
    client.0.lock().unwrap().mcp_support = Support::Http;
    let scopes = FakeScopes::new(grant);
    let policy = Arc::new(Policy(Mutex::new(flags)));
    let manager = manager.with_agent_mcp(connection(policy, 7316).with_scopes(scopes.clone()));
    (manager, registry, client, scopes)
}

fn bot_registration() -> AgentRegistration {
    AgentRegistration {
        internal: true,
        owner: Some(json!(domain::bot::SessionOwner {
            bot_id: domain::BotId::generate(),
            conversation_id: domain::ConversationId::generate(),
            workspace_id: "workspace-1".into(),
        })),
        ..AgentRegistration::default()
    }
}

fn subtree_registration() -> AgentRegistration {
    AgentRegistration {
        labels: BTreeMap::from([(
            domain::bot::BOT_SCOPE_LABEL.to_owned(),
            "workspace-1".to_owned(),
        )]),
        ..AgentRegistration::default()
    }
}

fn user_ait_spec() -> AgentSessionSpec {
    let mut spec = spec();
    spec.config.mcp_servers = Some(BTreeMap::from([
        (
            "ait".to_owned(),
            json!({"type":"http","url":"https://user.test/mcp","headers":{"Authorization":"Bearer user"}}),
        ),
        (
            "docs".to_owned(),
            json!({"type":"http","url":"https://docs.test/mcp"}),
        ),
    ]));
    spec
}

fn authorization(config: &StoredAgentConfig) -> Value {
    config.mcp_servers.as_ref().unwrap()["ait"]["headers"]["Authorization"].clone()
}

fn without_ait(config: &StoredAgentConfig) -> bool {
    config
        .mcp_servers
        .as_ref()
        .is_some_and(|servers| servers.contains_key("docs") && !servers.contains_key("ait"))
}

async fn complete_turn(manager: &mut AgentManager, client: &FakeClient, id: &str) {
    manager.send(id, "next").await.unwrap();
    client
        .0
        .lock()
        .unwrap()
        .events
        .push_back(AgentTurnEvent::Completed(Some("done".into())));
    manager.poll().await.unwrap();
}

#[tokio::test]
async fn bot_session_receives_scoped_credential_and_overrides_user_ait_entry() {
    let bot = "b".repeat(64);
    let (mut manager, registry, client, scopes) = scoped(
        Grant::Scoped(bot.clone(), true),
        json!({"mcp":{"injectIntoAgents":false}}),
    );
    let registration = bot_registration();
    manager
        .create("bot-session", &user_ait_spec(), registration.clone())
        .await
        .unwrap();
    let launched = client.0.lock().unwrap().create_specs[0].config.clone();
    assert_eq!(authorization(&launched), format!("Bearer {bot}"));
    assert_eq!(
        launched.mcp_servers.as_ref().unwrap()["ait"]["url"],
        "http://127.0.0.1:7316/mcp/agents"
    );
    let seen = scopes.subjects.lock().unwrap()[0].clone();
    assert!(seen.internal);
    assert_eq!(seen.owner, registration.owner);
    complete_turn(&mut manager, &client, "bot-session").await;
    assert_eq!(client.0.lock().unwrap().start_configs, vec![launched]);
    let persisted = serde_json::to_string(&registry.list().unwrap()).unwrap();
    assert!(!persisted.contains(&bot));
    assert!(persisted.contains("https://user.test/mcp"));

    let (mut manager, _, client, _) = scoped(
        Grant::Scoped(bot.clone(), true),
        json!({"mcp":{"enabled":false}}),
    );
    manager
        .create("bot-disabled", &user_ait_spec(), bot_registration())
        .await
        .unwrap();
    assert!(without_ait(
        &client.0.lock().unwrap().create_specs[0].config
    ));
}

#[tokio::test]
async fn resumed_subtree_agent_rebinds_scoped_credential() {
    let (mut manager, _, client, scopes) = scoped(Grant::Scoped("c".repeat(64), false), json!({}));
    manager
        .create("child", &spec(), subtree_registration())
        .await
        .unwrap();
    manager.close("child").await.unwrap();
    *scopes.grant.lock().unwrap() = Grant::Scoped("d".repeat(64), false);
    manager.resume("child").await.unwrap();
    let state = client.0.lock().unwrap();
    let first = authorization(&state.create_specs[0].config);
    let second = authorization(&state.resume_specs[0].config);
    assert_eq!(first, format!("Bearer {}", "c".repeat(64)));
    assert_eq!(second, format!("Bearer {}", "d".repeat(64)));
    assert_ne!(first, json!(format!("Bearer {TOKEN}")));
    let subjects = scopes.subjects.lock().unwrap();
    assert_eq!(subjects.len(), 2);
    assert!(
        subjects
            .iter()
            .all(|seen| seen.labels == subtree_registration().labels && !seen.internal)
    );
}

#[tokio::test]
async fn denied_bot_session_launches_without_ait_server() {
    let (mut manager, _, client, _) = scoped(Grant::Denied, json!({}));
    manager
        .create("bot-session", &user_ait_spec(), bot_registration())
        .await
        .unwrap();
    for _ in 0..2 {
        complete_turn(&mut manager, &client, "bot-session").await;
    }
    manager
        .configure(
            "bot-session",
            &ConfigPatch {
                model_id: provider_core::protocol::agent_config::NullableSetting::Set(
                    "gpt-7".into(),
                ),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let state = client.0.lock().unwrap();
    assert!(without_ait(&state.create_specs[0].config));
    assert_eq!(state.start_configs.len(), 2);
    assert!(state.start_configs.iter().all(without_ait));
    assert_eq!(state.validate_configs.len(), 1);
    assert!(state.validate_configs.iter().all(without_ait));
}

#[tokio::test]
async fn a_worker_on_an_additive_adapter_runs_its_report_tool_without_asking() {
    let report = json!({"preapproved":[{"kind":"mcp","server":"ait","tool":"report_progress"}]});
    for (support, expected) in [(Support::AdditiveHttp, Some(report)), (Support::Http, None)] {
        let (mut manager, registry, client, _) = scoped(Grant::Worker("w".repeat(64)), json!({}));
        let additive = matches!(support, Support::AdditiveHttp);
        client.0.lock().unwrap().mcp_support = support;
        manager
            .create("worker", &spec(), subtree_registration())
            .await
            .unwrap();
        complete_turn(&mut manager, &client, "worker").await;
        manager
            .configure(
                "worker",
                &ConfigPatch {
                    model_id: provider_core::protocol::agent_config::NullableSetting::Set(
                        "gpt-7".into(),
                    ),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let state = client.0.lock().unwrap();
        let launched = [
            &state.create_specs[0].config,
            &state.start_configs[0],
            &state.validate_configs[0],
        ];
        for config in launched {
            assert_eq!(config.tool_policy, expected, "additive={additive}");
        }
        let stored = registry.get("worker").unwrap().unwrap().config.unwrap();
        assert_eq!(stored.tool_policy, None);
        assert_eq!(stored.model.as_deref(), Some("gpt-7"));
    }
}

#[tokio::test]
async fn subtree_without_injection_strips_user_ait_entry() {
    let (mut manager, _, client, scopes) = scoped(
        Grant::Scoped("c".repeat(64), false),
        json!({"mcp":{"injectIntoAgents":false}}),
    );
    manager
        .create("child", &user_ait_spec(), subtree_registration())
        .await
        .unwrap();
    complete_turn(&mut manager, &client, "child").await;
    let state = client.0.lock().unwrap();
    assert!(without_ait(&state.create_specs[0].config));
    assert!(state.start_configs.iter().all(without_ait));
    assert_eq!(scopes.calls(), ["access:child", "release:child"]);
}

#[tokio::test]
async fn closing_or_archiving_a_scoped_session_releases_its_credential() {
    let (mut manager, registry, client, scopes) =
        scoped(Grant::Scoped("c".repeat(64), false), json!({}));
    manager
        .create("child", &spec(), subtree_registration())
        .await
        .unwrap();
    manager.close("child").await.unwrap();
    assert_eq!(scopes.calls(), ["access:child", "release:child"]);
    manager.resume("child").await.unwrap();
    assert_eq!(
        scopes.calls(),
        ["access:child", "release:child", "access:child"]
    );

    manager
        .create("other", &spec(), subtree_registration())
        .await
        .unwrap();
    registry
        .update("other", &|current| {
            let mut next = current.clone();
            next.archived_at = Some("2026-11-06T00:00:00Z".into());
            next
        })
        .unwrap();
    manager.reconcile().await.unwrap();
    assert!(
        scopes
            .calls()
            .ends_with(&["access:other".to_owned(), "release:other".to_owned()])
    );

    manager.send("child", "fails").await.unwrap();
    client
        .0
        .lock()
        .unwrap()
        .events
        .push_back(AgentTurnEvent::Failed);
    manager.poll().await.unwrap();
    assert!(manager.live_snapshot("child").is_none());
    assert_eq!(scopes.calls().last().unwrap(), "release:child");

    client.0.lock().unwrap().create_error = Some(AgentSessionError::Failed);
    assert!(
        manager
            .create("broken", &spec(), subtree_registration())
            .await
            .is_err()
    );
    assert!(
        scopes
            .calls()
            .ends_with(&["access:broken".to_owned(), "release:broken".to_owned()])
    );
}

#[tokio::test]
async fn scoped_sessions_without_http_mcp_strip_user_ait_entry() {
    for (id, registration, bot_session) in [
        ("bot-session", bot_registration(), true),
        ("child", subtree_registration(), false),
    ] {
        let (mut manager, _, client, scopes) =
            scoped(Grant::Scoped("c".repeat(64), bot_session), json!({}));
        client.0.lock().unwrap().mcp_support = Support::Unsupported;
        manager
            .create(id, &user_ait_spec(), registration)
            .await
            .unwrap();
        complete_turn(&mut manager, &client, id).await;
        let state = client.0.lock().unwrap();
        assert!(without_ait(&state.create_specs[0].config));
        assert_eq!(state.start_configs.len(), 1);
        assert!(state.start_configs.iter().all(without_ait));
        assert_eq!(
            scopes.calls(),
            [format!("access:{id}"), format!("release:{id}")]
        );
    }
}

#[tokio::test]
async fn restoring_a_live_scoped_agent_releases_before_reissuing() {
    let (mut manager, _, client, scopes) = scoped(Grant::Scoped("c".repeat(64), false), json!({}));
    manager
        .create("child", &spec(), subtree_registration())
        .await
        .unwrap();
    *scopes.grant.lock().unwrap() = Grant::Scoped("d".repeat(64), false);
    manager
        .restore(
            "child",
            &crate::protocol::resume::Overrides {
                title: provider_core::protocol::agent_config::NullableSetting::Set(
                    "Renamed".into(),
                ),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        scopes.calls(),
        ["access:child", "release:child", "access:child"]
    );
    assert_eq!(
        authorization(&client.0.lock().unwrap().resume_specs[0].config),
        format!("Bearer {}", "d".repeat(64))
    );
}

#[tokio::test]
async fn imported_records_pass_their_labels_to_scoped_access() {
    let (mut manager, _, client, scopes) = scoped(Grant::Scoped("c".repeat(64), false), json!({}));
    let mut imported = stored_record();
    imported.id = "imported".into();
    imported.labels = subtree_registration().labels;
    manager.restore_new(imported).await.unwrap();
    assert_eq!(
        scopes.subjects.lock().unwrap()[0].labels,
        subtree_registration().labels
    );
    assert_eq!(
        authorization(&client.0.lock().unwrap().resume_specs[0].config),
        format!("Bearer {}", "c".repeat(64))
    );
}

#[tokio::test]
async fn failed_registration_releases_the_issued_credential() {
    let (mut manager, registry, client, scopes) =
        scoped(Grant::Scoped("c".repeat(64), false), json!({}));
    client.0.lock().unwrap().fail_info = true;
    assert!(
        manager
            .create("invalid", &spec(), subtree_registration())
            .await
            .is_err()
    );
    client.0.lock().unwrap().fail_info = false;
    registry.0.lock().unwrap().fail_upsert = true;
    assert!(
        manager
            .create("unstored", &spec(), subtree_registration())
            .await
            .is_err()
    );
    assert_eq!(
        scopes.calls(),
        [
            "access:invalid",
            "release:invalid",
            "access:unstored",
            "release:unstored"
        ]
    );
    assert!(manager.live_snapshot("invalid").is_none());
}
