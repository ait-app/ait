use super::fixture::{Fixture, next};
use crate::ports::agent_session::{AgentClient, AgentResumePurpose, AgentTurnEvent};
use serde_json::json;

#[tokio::test]
async fn native_permissions_questions_order_and_resume_use_exact_session() {
    let mut fixture = Fixture::new().await;
    fixture.spec.config.mode_id = Some("read-only".into());
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    assert_eq!(
        fixture.client.settings(&fixture.spec.config)["availableModes"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        fixture.requests("commands/execute")[0]["line"],
        "/permission read-only"
    );
    fixture.spec.config.mode_id = Some("workspace-write".into());
    fixture.spec.config.thinking_option_id = Some("high".into());
    session
        .start_turn("hello", &fixture.spec.config)
        .await
        .unwrap();
    assert!(matches!(
        next(session.as_mut()).await,
        AgentTurnEvent::RuntimeInfo(_)
    ));
    assert_eq!(
        fixture.requests("session/selectModel")[0]["request"]["reasoningEffort"],
        "high"
    );
    fixture.history(1, "turn/start", json!({"turn":1}));
    // Different logical streams may deliver the approval before the durable tool record.
    fixture.interaction(json!({"type":"waterfall","agentId":"session","eventId":"approval","event":"approval/request","request":{"toolName":"bash","callId":"call"}}));
    fixture.history(
        2,
        "tool/call",
        json!({"turn":1,"callId":"call","name":"bash","arguments":"{\"command\":\"pwd\"}"}),
    );
    assert!(matches!(
        next(session.as_mut()).await,
        AgentTurnEvent::Progress { .. }
    ));
    let AgentTurnEvent::PermissionRequested(request) = next(session.as_mut()).await else {
        panic!("approval");
    };
    assert_eq!(request["input"]["command"], "pwd");
    let id = request["id"].as_str().unwrap();
    session
        .respond_permission(id, &json!({"behavior":"allow"}))
        .await
        .unwrap();
    assert!(
        session
            .respond_permission(id, &json!({"behavior":"allow"}))
            .await
            .is_err()
    );
    assert!(matches!(
        next(session.as_mut()).await,
        AgentTurnEvent::PermissionResolved(_)
    ));
    assert_eq!(
        fixture.requests("$events/result")[0]["outcome"]["value"],
        "allowed-once"
    );
    fixture.history(3,"tool/result",json!({"turn":1,"message":{"content":[{"type":"tool-result","toolCallId":"call","content":[{"type":"text","text":"/workspace"}]}]}}));
    fixture.history(
        4,
        "assistant/message",
        json!({"turn":1,"message":{"id":"answer","content":[{"type":"text","text":"Done 中文"}]}}),
    );
    let AgentTurnEvent::Timeline(tool) = next(session.as_mut()).await else {
        panic!("tool must precede answer");
    };
    assert_eq!(tool.item["type"], "tool_call");
    assert!(matches!(
        next(session.as_mut()).await,
        AgentTurnEvent::Progress { .. }
    ));
    assert!(matches!(
        next(session.as_mut()).await,
        AgentTurnEvent::Timeline(_)
    ));
    answer_question(&fixture, session.as_mut()).await;
    fixture.history(
        5,
        "turn/end",
        json!({"turn":1,"reason":{"kind":"completed"}}),
    );
    assert!(matches!(
        next(session.as_mut()).await,
        AgentTurnEvent::Completed(Some(_))
    ));
    resume_without_replay(&fixture, session.as_mut()).await;
}

#[tokio::test]
async fn cancellation_expires_questions_and_allows_a_following_turn() {
    let fixture = Fixture::new().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    let turn = session
        .start_turn("question", &fixture.spec.config)
        .await
        .unwrap();
    let _ = next(session.as_mut()).await;
    fixture.history(1, "turn/start", json!({"turn":1}));
    fixture.interaction(json!({"type":"waterfall","agentId":"session","eventId":"question","event":"user-questions/request","request":{"questions":[{"id":"q","question":"Why?"}]}}));
    let AgentTurnEvent::PermissionRequested(question) = next(session.as_mut()).await else {
        panic!("question");
    };
    session.cancel_turn(&turn).await.unwrap();
    fixture.interaction(json!({"type":"cancel","eventId":"question"}));
    assert!(matches!(
        next(session.as_mut()).await,
        AgentTurnEvent::PermissionResolved(_)
    ));
    assert!(
        session
            .respond_permission(
                question["id"].as_str().unwrap(),
                &json!({"behavior":"deny"})
            )
            .await
            .is_err()
    );
    fixture.history(2, "turn/end", json!({"turn":1,"reason":{"kind":"aborted"}}));
    assert!(matches!(
        next(session.as_mut()).await,
        AgentTurnEvent::Cancelled
    ));
    session
        .start_turn("continue", &fixture.spec.config)
        .await
        .unwrap();
    let _ = next(session.as_mut()).await;
    fixture.history(3, "turn/start", json!({"turn":2}));
    fixture.history(
        4,
        "turn/end",
        json!({"turn":2,"reason":{"kind":"completed"}}),
    );
    assert!(matches!(
        next(session.as_mut()).await,
        AgentTurnEvent::Completed(_)
    ));
    assert_eq!(fixture.requests("session/prompt").len(), 2);
    session.close().await.unwrap();
}

async fn answer_question(
    fixture: &Fixture,
    session: &mut dyn crate::ports::agent_session::AgentSession,
) {
    fixture.interaction(json!({"type":"waterfall","agentId":"session","eventId":"question","event":"user-questions/request","request":{"questions":[{"id":"choice","question":"Continue?","options":[{"label":"Yes"}]}]}}));
    let AgentTurnEvent::PermissionRequested(question) = next(session).await else {
        panic!("question");
    };
    session
        .respond_permission(
            question["id"].as_str().unwrap(),
            &json!({"behavior":"allow","updatedInput":{"answers":{"choice":"Yes"}}}),
        )
        .await
        .unwrap();
    assert!(matches!(
        next(session).await,
        AgentTurnEvent::PermissionResolved(_)
    ));
    assert_eq!(
        fixture.requests("$events/result")[1]["outcome"]["value"],
        json!({"answers":[{"id":"choice","selected":["Yes"]}]})
    );
}

#[tokio::test]
async fn denial_foreign_requests_and_context_updates_preserve_native_authority() {
    let fixture = Fixture::new().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .start_turn("test", &fixture.spec.config)
        .await
        .unwrap();
    let _ = next(session.as_mut()).await;
    fixture.interaction(json!({"type":"waterfall","agentId":"foreign","eventId":"foreign","event":"approval/request","request":{"toolName":"bash"}}));
    fixture.interaction(json!({"type":"waterfall","agentId":"session","eventId":"approval","event":"approval/request","request":{"toolName":"bash","reason":"Native hook asks"}}));
    let AgentTurnEvent::PermissionRequested(approval) = next(session.as_mut()).await else {
        panic!("approval")
    };
    session
        .respond_permission(
            approval["id"].as_str().unwrap(),
            &json!({"behavior":"deny","interrupt":true}),
        )
        .await
        .unwrap();
    assert!(matches!(
        next(session.as_mut()).await,
        AgentTurnEvent::PermissionResolved(_)
    ));
    assert_eq!(fixture.requests("session/cancel").len(), 1);
    fixture.send(json!({"type":"item","streamId":"control","value":{"type":"projection","sessionId":"session","key":"contextPressure","seq":1,"value":{"projectedTokens":42,"contextWindow":1000}}}));
    let AgentTurnEvent::Usage(usage) = next(session.as_mut()).await else {
        panic!("usage")
    };
    assert_eq!(usage.context_window_used_tokens, Some(42));
    let requests = fixture.requests("$events/result");
    assert!(
        requests
            .iter()
            .any(|request| request["eventId"] == "foreign" && request["outcome"]["kind"] == "next")
    );
    assert!(requests.iter().any(
        |request| request["eventId"] == "approval" && request["outcome"]["value"] == "rejected"
    ));
    session.close().await.unwrap();
}

#[tokio::test]
async fn unknown_modes_and_invalid_prompt_do_not_admit_input() {
    let fixture = Fixture::new().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    let mut config = fixture.spec.config.clone();
    config.mode_id = Some("invented".into());
    assert!(session.start_turn("hello", &config).await.is_err());
    assert!(fixture.requests("commands/execute").is_empty());
    assert!(fixture.requests("session/prompt").is_empty());
    session.close().await.unwrap();
}

#[tokio::test]
async fn history_gap_fails_without_resending_input() {
    let fixture = Fixture::new().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .start_turn("once", &fixture.spec.config)
        .await
        .unwrap();
    let _ = next(session.as_mut()).await;
    fixture.history(2, "turn/start", json!({"turn":1}));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if session.poll_turn().is_err() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(fixture.requests("session/prompt").len(), 1);
    session.close().await.unwrap();
}

async fn resume_without_replay(
    fixture: &Fixture,
    session: &mut dyn crate::ports::agent_session::AgentSession,
) {
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let mut resumed = fixture
        .client
        .resume_session(&handle, &fixture.spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    assert_eq!(
        fixture.requests("session/prompt").len(),
        1,
        "resume must not replay input"
    );
    assert_eq!(resumed.persistence().unwrap().session_id, handle.session_id);
    resumed.close().await.unwrap();
}

#[tokio::test]
async fn image_output_is_materialized_before_following_text_and_completion() {
    let fixture = Fixture::new().await;
    let client = fixture
        .client
        .clone()
        .with_image_directory(std::path::Path::new(&fixture.spec.cwd).join("images"));
    let mut session = client.create_session(&fixture.spec).await.unwrap();
    session
        .start_turn("image", &fixture.spec.config)
        .await
        .unwrap();
    let _ = next(session.as_mut()).await;
    fixture.history(1, "turn/start", json!({"turn":1}));
    fixture.history(2,"assistant/message",json!({"turn":1,"message":{"id":"answer","content":[{"type":"image","attachment":{"attachmentId":"image","mediaType":"image/png"}},{"type":"text","text":"caption"}]}}));
    fixture.history(
        3,
        "turn/end",
        json!({"turn":1,"reason":{"kind":"completed"}}),
    );
    assert!(matches!(
        next(session.as_mut()).await,
        AgentTurnEvent::Progress { .. }
    ));
    assert!(matches!(
        next(session.as_mut()).await,
        AgentTurnEvent::Progress { .. }
    ));
    let AgentTurnEvent::Timeline(item) = next(session.as_mut()).await else {
        panic!("image")
    };
    assert!(item.item["text"].as_str().unwrap().starts_with("![Image]("));
    assert!(item.item["text"].as_str().unwrap().ends_with("caption"));
    assert_eq!(
        std::fs::read_dir(std::path::Path::new(&fixture.spec.cwd).join("images"))
            .unwrap()
            .count(),
        1
    );
    assert!(matches!(
        next(session.as_mut()).await,
        AgentTurnEvent::Completed(_)
    ));
    session.close().await.unwrap();
}

#[tokio::test]
async fn separate_permission_catalog_preserves_native_choices() {
    let mut fixture = Fixture::new().await;
    fixture.set_snapshot_fields(json!({"projections":{"values":{
        "permissions":{"currentValue":"workspace-write"}
    }}}));
    fixture.spec.config.mode_id = Some("custom-policy".into());
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    assert_eq!(fixture.requests("permissionPresets/catalog").len(), 1);
    assert_eq!(
        fixture.requests("commands/execute")[0]["line"],
        "/permission custom-policy"
    );
    assert_eq!(
        session.runtime_info().await.unwrap().mode_id.as_deref(),
        Some("custom-policy")
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn embedded_permission_catalog_needs_no_new_endpoint() {
    let fixture = Fixture::new().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    assert!(fixture.requests("permissionPresets/catalog").is_empty());
    session.close().await.unwrap();
}

#[tokio::test]
async fn malformed_permission_projection_is_not_replaced_with_defaults() {
    for permissions in [
        json!({}),
        json!({"currentValue":"read-only","options":null}),
    ] {
        let fixture = Fixture::new().await;
        fixture.set_snapshot_fields(json!({"projections":{"values":{"permissions":permissions}}}));
        assert!(fixture.client.create_session(&fixture.spec).await.is_err());
        assert!(fixture.requests("permissionPresets/catalog").is_empty());
        assert!(fixture.requests("commands/execute").is_empty());
    }
}
