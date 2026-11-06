use super::fixture::{Fixture, next};
use crate::ports::agent_session::{AgentClient, AgentSessionError, AgentTurnEvent};
use crate::protocol::prompt::AgentPrompt;
use domain::agent_runtime::registry::AgentRuntimeRegistry;
use serde_json::json;

#[tokio::test]
async fn draft_catalog_uses_native_agent_composition_without_submitting_input() {
    let fixture = Fixture::new().await;
    fixture.reply(
        "commands/list",
        json!([{"name":"plugin_action","description":"Native draft action",
            "definitionId":"plugin:action","input":{"hint":"<value>"}}]),
    );
    let commands = fixture.client.commands(&fixture.spec).await.unwrap();
    assert_eq!(commands[0]["name"], "plugin_action");
    assert_eq!(commands[0]["argumentHint"], "<value>");
    assert_eq!(commands[0]["definitionId"], "plugin:action");
    assert_eq!(fixture.requests("commands/list")[0]["agentId"], "session");
    assert!(fixture.requests("session/prompt").is_empty());
    assert!(fixture.requests("commands/execute").is_empty());
}

#[tokio::test]
async fn initial_slash_input_uses_native_command_admission_instead_of_a_model_prompt() {
    let fixture = Fixture::new().await;
    let mut host = crate::test_support::Fixture::new();
    host.cwd = std::path::PathBuf::from(&fixture.spec.cwd);
    let (execution, registry) = crate::test_support::worker_with_client(
        &host,
        model::creation::Creations::default(),
        Box::new(fixture.client.clone()),
    );
    let environment: std::collections::BTreeMap<_, _> =
        fixture.client.environment.entries().collect();
    let created = execution
        .execute(
            "agent.create.request",
            json!({"config":{"provider":"deepseek-harness","cwd":host.cwd},
            "initialPrompt":"/goal","clientMessageId":"initial-command","env":environment}),
        )
        .await;
    assert!(
        created.is_ok(),
        "{created:?}; catalog={}, sessions={}, commands={}",
        fixture.requests("session/modelCatalog").len(),
        fixture.requests("session/create").len(),
        fixture.requests("commands/execute").len()
    );
    let created = created.unwrap();
    assert_eq!(created["status"], "agent_created", "{created:?}");
    assert_eq!(fixture.requests("commands/execute")[0]["line"], "/goal");
    assert!(fixture.requests("session/prompt").is_empty());
    let listed = crate::test_support::request(&execution, "agent.list.request", json!({})).await;
    assert_eq!(
        listed["entries"][0]["agent"]["features"][0]["id"],
        "permission_preset"
    );
    fixture.history(1, "permission/preset", json!({"preset":"read-only"}));
    let changed = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let listed =
                crate::test_support::request(&execution, "agent.list.request", json!({})).await;
            if listed["entries"][0]["agent"]["features"][0]["value"] == "read-only" {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    if changed.is_err() {
        let current = crate::test_support::request(
            &execution,
            "agent.get.request",
            json!({"agentId":created["agentId"]}),
        )
        .await;
        panic!("native permission did not reach directory controls: {current:?}");
    }
    let current = registry
        .get(created["agentId"].as_str().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(
        current.config.unwrap().feature_values.unwrap()["permission_preset"],
        json!("read-only")
    );
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn command_and_catalog_errors_do_not_fail_or_replace_the_current_model_reply() {
    let fixture = Fixture::new().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .start_turn("ordinary message", &fixture.spec.config)
        .await
        .unwrap();
    fixture.history(1, "turn/start", json!({"turn":7}));
    fixture.fail("commands/list", "500");
    assert!(session.refresh_commands().await.is_err());
    assert!(session.commands().is_some());
    assert!(session.failure_message().is_none());

    let release = fixture.hold_command();
    fixture.fail("commands/execute", "500");
    let prompt = AgentPrompt::text("/goal status");
    let (accepted, ()) = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        tokio::join!(session.out_of_band(&prompt), async {
            while fixture.requests("commands/execute").is_empty() {
                tokio::task::yield_now().await;
            }
            fixture.history(
                2,
                "command/run",
                json!({"commandId":"goal-1","name":"goal","source":{"kind":"user"}}),
            );
        })
    })
    .await
    .unwrap();
    accepted.unwrap();
    release.send(()).unwrap();
    let notice = loop {
        match next(session.as_mut()).await {
            AgentTurnEvent::Timeline(item) if item.item["type"] == "error" => break item,
            AgentTurnEvent::Failed | AgentTurnEvent::Cancelled | AgentTurnEvent::Completed(_) => {
                panic!("control failure ended the model turn")
            }
            _ => {}
        }
    };
    assert!(notice.key.starts_with("host:dsh:command-response:"));
    assert!(!notice.item.to_string().contains("fixture-secret"));
    assert!(session.failure_message().is_none());
    fixture.history(
        3,
        "assistant/message",
        json!({"turn":7,"message":{"id":"answer",
        "content":[{"type":"text","text":"Model kept running"}]}}),
    );
    fixture.history(
        4,
        "command/done",
        json!({"commandId":"goal-1","kind":"error","text":"Goal action failed"}),
    );
    fixture.history(
        5,
        "turn/end",
        json!({"turn":7,"reason":{"kind":"completed"}}),
    );
    loop {
        match next(session.as_mut()).await {
            AgentTurnEvent::Completed(reply) => {
                assert_eq!(reply.as_deref(), Some("Model kept running"));
                break;
            }
            AgentTurnEvent::Failed | AgentTurnEvent::Cancelled => {
                panic!("control failure ended the model turn")
            }
            _ => {}
        }
    }
    session.close().await.unwrap();
}

#[tokio::test]
async fn session_command_registry_is_scoped_and_empty_native_updates_replace_the_catalog() {
    let fixture = Fixture::new().await;
    fixture.reply(
        "commands/list",
        json!([{"name":"plugin_action","description":"Plugin-owned action",
        "definitionId":"plugin:action","input":{"hint":"<objective>","attachments":true}}]),
    );
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    assert_eq!(session.commands().unwrap()[0]["name"], "plugin_action");
    assert_eq!(
        session.commands().unwrap()[0]["definitionId"],
        "plugin:action"
    );
    assert_eq!(
        session.commands().unwrap()[0]["argumentHint"],
        "<objective>"
    );
    assert_eq!(fixture.requests("commands/list")[0]["agentId"], "session");
    fixture.reply("commands/list", json!([]));
    fixture.interaction(json!({"type":"emit","event":"commands/change","args":[]}));
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while !session.commands().unwrap().is_empty() {
            session.poll_turn().unwrap();
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(fixture.requests("commands/list").len(), 2);
    assert!(fixture.requests("session/prompt").is_empty());
    session.close().await.unwrap();
}

#[tokio::test]
async fn host_commands_keep_raw_input_and_do_not_interrupt_the_current_model_turn() {
    let fixture = Fixture::new().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    let turn = session
        .start_turn("ordinary message", &fixture.spec.config)
        .await
        .unwrap();
    let line = "/goal edit \n  preserve this objective  ";
    assert!(fixture.client.handles_out_of_band(line));
    session.out_of_band(&AgentPrompt::text(line)).await.unwrap();
    let requests = fixture.requests("commands/execute");
    assert_eq!(requests[0]["agentId"], "session");
    assert_eq!(requests[0]["line"], line);
    assert_eq!(requests[0]["submittedAttachments"], json!([]));
    assert_eq!(fixture.requests("session/prompt").len(), 1);
    assert!(fixture.requests("session/cancel").is_empty());
    session.cancel_turn(&turn).await.unwrap();
    session.close().await.unwrap();
}

#[tokio::test]
async fn keyless_native_sessions_keep_control_access_but_reject_unadvertised_model_selections() {
    let fixture = Fixture::new().await;
    fixture.reply(
        "session/modelCatalog",
        json!({"default":{"provider":"native","model":"default"},"groups":[]}),
    );
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .out_of_band(&AgentPrompt::text("/goal"))
        .await
        .unwrap();
    assert!(fixture.requests("session/selectModel").is_empty());
    assert!(fixture.requests("session/prompt").is_empty());
    session.close().await.unwrap();
    let mut explicit = fixture.spec.clone();
    explicit.config.model = Some(json!(["native", "unadvertised"]).to_string());
    assert!(matches!(
        fixture.client.create_session(&explicit).await,
        Err(AgentSessionError::Rejected)
    ));
    assert!(fixture.requests("session/selectModel").is_empty());
}

#[tokio::test]
async fn unmatched_native_commands_are_rejected_instead_of_becoming_model_prompts() {
    let fixture = Fixture::new().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    fixture.fail("commands/execute", "missing");
    assert!(matches!(
        session.out_of_band(&AgentPrompt::text("/missing")).await,
        Err(AgentSessionError::Rejected)
    ));
    assert!(fixture.requests("session/prompt").is_empty());
    session.close().await.unwrap();
}

#[tokio::test]
async fn command_outcomes_are_live_and_recoverable_without_a_model_turn() {
    let fixture = Fixture::new().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    fixture.history(
        1,
        "command/run",
        json!({"commandId":"goal-1","name":"goal","args":" pause","source":{"kind":"user"}}),
    );
    fixture.history(
        2,
        "command/done",
        json!({"commandId":"goal-1","kind":"success","text":"Goal paused"}),
    );
    let input = match next(session.as_mut()).await {
        AgentTurnEvent::Timeline(item) => item,
        event => panic!("expected native command input, got {event:?}"),
    };
    assert_eq!(input.item["text"], "/goal pause");
    let live = match next(session.as_mut()).await {
        AgentTurnEvent::Timeline(item) => item,
        event => panic!("expected command outcome, got {event:?}"),
    };
    assert_eq!(live.item["text"], "Goal paused");
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let history = fixture
        .client
        .history(&handle, &fixture.spec.cwd)
        .await
        .unwrap();
    assert!(
        history
            .iter()
            .any(|item| item.key == live.key && item.item["text"] == "Goal paused")
    );
    assert_eq!(history[0], input);
}

#[tokio::test]
#[ignore = "requires AIT_TEST_DSH_BIN; isolated native command discovery and /goal status, no inference"]
async fn installed_host_registry_and_native_goal_status_work_without_a_model_prompt() {
    let binary = std::env::var_os("AIT_TEST_DSH_BIN").expect("set AIT_TEST_DSH_BIN");
    let directory = tempfile::tempdir().unwrap();
    let mut client = super::DeepSeekHarnessClient::new(binary.into());
    client.environment = super::AgentEnvironment::try_from(std::collections::BTreeMap::from([(
        "DSH_HOME".into(),
        directory.path().to_str().unwrap().into(),
    )]))
    .unwrap();
    let spec = crate::ports::agent_session::AgentSessionSpec {
        confinement: None,
        provider: super::PROVIDER.into(),
        cwd: directory.path().to_str().unwrap().into(),
        config: super::StoredAgentConfig::default(),
    };
    assert!(
        client
            .commands(&spec)
            .await
            .unwrap()
            .iter()
            .any(|command| command["name"] == "goal")
    );
    assert!(
        !directory.path().join("sessions").exists(),
        "draft command discovery must not materialize native session storage"
    );
    let mut preview = super::super::runtime::Runtime::open(&client, &spec.cwd)
        .await
        .unwrap();
    let native = preview
        .api
        .call("session/list", json!({"_request":{}}))
        .await
        .unwrap();
    assert_eq!(native["items"], json!([]));
    preview.close().await.unwrap();
    let mut session = client.create_session(&spec).await.unwrap();
    assert!(
        session
            .commands()
            .unwrap()
            .iter()
            .any(|command| command["name"] == "goal")
    );
    session
        .out_of_band(&AgentPrompt::text("/goal"))
        .await
        .unwrap();
    loop {
        if let AgentTurnEvent::Timeline(item) = next(session.as_mut()).await
            && item.item["type"] == "provider_command"
            && item.item["text"]
                .as_str()
                .is_some_and(|text| text.to_lowercase().contains("goal"))
        {
            break;
        }
    }
    session.close().await.unwrap();
}

#[tokio::test]
async fn autonomous_native_turns_started_by_goal_drivers_reach_the_host_lifecycle() {
    let fixture = Fixture::new().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    fixture.history(1, "turn/start", json!({"turn":11}));
    assert_eq!(
        next(session.as_mut()).await,
        AgentTurnEvent::Started("dsh:session:11".into())
    );
    fixture.history(
        2,
        "assistant/message",
        json!({"turn":11,"message":{"id":"goal-answer",
        "content":[{"type":"text","text":"Native goal progress"}]}}),
    );
    fixture.history(
        3,
        "turn/end",
        json!({"turn":11,"reason":{"kind":"completed"}}),
    );
    loop {
        if let AgentTurnEvent::Completed(reply) = next(session.as_mut()).await {
            assert_eq!(reply.as_deref(), Some("Native goal progress"));
            break;
        }
    }
    session.close().await.unwrap();
}
