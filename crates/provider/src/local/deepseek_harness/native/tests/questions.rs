use super::fixture::{Fixture, next};
use crate::ports::agent_session::{AgentClient, AgentTurnEvent};
use serde_json::json;

#[tokio::test]
#[ignore = "requires AIT_TEST_DSH_BIN; isolated DSH home, no sessions or model requests"]
async fn installed_host_accepts_void_event_result_acknowledgement_without_model_execution() {
    let binary = std::env::var_os("AIT_TEST_DSH_BIN").expect("set AIT_TEST_DSH_BIN");
    let directory = tempfile::tempdir().unwrap();
    let cwd = directory.path().to_str().unwrap();
    let mut client = crate::local::deepseek_harness::DeepSeekHarnessClient::new(binary.into());
    client.environment = std::collections::BTreeMap::from([("DSH_HOME".into(), cwd.into())])
        .try_into()
        .unwrap();
    let mut runtime = super::super::runtime::Runtime::open(&client, cwd)
        .await
        .unwrap();
    // An unknown event is a native no-op, acknowledged through the same void RPC as real answers.
    let result = runtime
        .api
        .call(
            "$events/result",
            json!({"clientId":runtime.client_id,"eventId":uuid::Uuid::new_v4().to_string(),
                "outcome":{"kind":"next"}}),
        )
        .await;
    runtime.close().await.unwrap();
    assert_eq!(result.unwrap(), serde_json::Value::Null);
    assert!(!directory.path().join("sessions").exists());
}

#[tokio::test]
async fn custom_question_answer_with_void_acknowledgement_allows_turn_to_continue() {
    let fixture = Fixture::new().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .start_turn("Explain the warnings", &fixture.spec.config)
        .await
        .unwrap();
    let _ = next(session.as_mut()).await;
    fixture.history(1, "turn/start", json!({"turn":1}));
    fixture.interaction(json!({
        "type":"waterfall", "agentId":"session", "eventId":"question",
        "event":"user-questions/request", "request":{"questions":[{
            "id":"direction", "question":"How should we proceed?",
            "options":[{"label":"Add an audit mode"}]
        }]}
    }));
    let AgentTurnEvent::PermissionRequested(question) = next(session.as_mut()).await else {
        panic!("native question should be presented");
    };
    let answer = "这些 warning 的意思是什么？";
    session
        .respond_permission(
            question["id"].as_str().unwrap(),
            &json!({"behavior":"allow","updatedInput":{"answers":{"direction":[answer]}}}),
        )
        .await
        .expect("the native void acknowledgement must not fail an accepted answer");
    assert!(session.pending_permissions().is_empty());
    assert!(matches!(
        next(session.as_mut()).await,
        AgentTurnEvent::PermissionResolved(id) if id == question["id"]
    ));
    let requests = fixture.requests("$events/result");
    assert_eq!(
        requests.len(),
        1,
        "an accepted answer must not be sent twice"
    );
    assert_eq!(
        requests[0]["outcome"],
        json!({"kind":"result","value":{"answers":[{
            "id":"direction","selected":[],"custom":answer
        }]}})
    );
    fixture.history(
        2,
        "assistant/message",
        json!({"turn":1,"message":{"id":"answer","content":[{
            "type":"text","text":"Here is what the warnings mean."
        }]}}),
    );
    assert!(matches!(
        next(session.as_mut()).await,
        AgentTurnEvent::Progress { .. }
    ));
    assert!(matches!(
        next(session.as_mut()).await,
        AgentTurnEvent::Timeline(_)
    ));
    fixture.history(
        3,
        "turn/end",
        json!({"turn":1,"reason":{"kind":"completed"}}),
    );
    assert!(matches!(
        next(session.as_mut()).await,
        AgentTurnEvent::Completed(_)
    ));
    session.close().await.unwrap();
}
