use super::*;
use std::future::Future;

async fn settle<T>(
    session: &mut dyn AgentSession,
    future: impl Future<Output = Result<T, AgentSessionError>>,
) -> (T, Vec<AgentTurnEvent>) {
    tokio::pin!(future);
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut events = Vec::new();
        loop {
            if let Some(event) = session.poll_turn().unwrap() {
                events.push(event);
            }
            tokio::select! {
                value=&mut future => {
                    while let Some(event)=session.poll_turn().unwrap() { events.push(event); }
                    break (value.unwrap(),events);
                },
                ()=tokio::time::sleep(Duration::from_millis(2))=>{}
            }
        }
    })
    .await
    .unwrap()
}

async fn wait_for_side_completion(session: &mut dyn AgentSession) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            session.poll_turn().unwrap();
            if session.ui_state().unwrap()["sideQuestions"]["runningId"].is_null() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
}

async fn command(session: &mut dyn AgentSession, text: &str) -> Vec<AgentTurnEvent> {
    let prompt = AgentPrompt {
        client_message_id: Some(uuid::Uuid::new_v4().to_string()),
        ..AgentPrompt::text(text)
    };
    session.out_of_band(&prompt).await.unwrap();
    let receipt = session.take_command_completion().unwrap();
    settle(session, receipt).await.1
}

#[tokio::test]
async fn queries_and_rejected_tui_commands_leave_a_running_main_turn_intact() {
    let (root, client, spec) = fixture();
    std::fs::create_dir_all(root.path().join("agent")).unwrap();
    std::fs::write(root.path().join("agent/side-commands"), "").unwrap();
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("wait", &spec.config).await.unwrap();
    let events = command(session.as_mut(), "/usage").await;
    assert!(events.iter().any(|event|matches!(event,AgentTurnEvent::Timeline(entry)
        if entry.item["type"]=="provider_command" && entry.item["text"]=="Native usage: 123 tokens")));
    assert!(!events.iter().any(|event| matches!(
        event,
        AgentTurnEvent::Completed(_) | AgentTurnEvent::Cancelled | AgentTurnEvent::Failed
    )));
    let before = std::fs::read(root.path().join("agent/calls.jsonl")).unwrap();
    session
        .out_of_band(&AgentPrompt::text("/model other/model"))
        .await
        .unwrap();
    assert!(session.take_command_completion().is_none());
    let event = next(session.as_mut()).await.unwrap();
    assert!(
        matches!(event,AgentTurnEvent::Timeline(entry) if entry.item["command"]=="model" && entry.item["status"]=="error")
    );
    assert_eq!(
        std::fs::read(root.path().join("agent/calls.jsonl")).unwrap(),
        before
    );
    assert_eq!(
        session.handles_out_of_band("/status"),
        Some(false),
        "a real extension owns this TUI-only name"
    );
    std::fs::write(root.path().join("agent/finish-wait"), "").unwrap();
    let finished = complete(session.as_mut()).await;
    assert_eq!(
        finished.last(),
        Some(&AgentTurnEvent::Completed(Some("Hello world".to_owned())))
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn side_questions_stream_cancel_and_follow_up_without_settling_the_foreground() {
    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("wait", &spec.config).await.unwrap();
    let events = command(session.as_mut(), "/btw hold question").await;
    assert!(!events.iter().any(|event| matches!(
        event,
        AgentTurnEvent::Completed(_) | AgentTurnEvent::Cancelled | AgentTurnEvent::Failed
    )));
    let ui = session.ui_state().unwrap();
    let id = ui["sideQuestions"]["runningId"]
        .as_str()
        .unwrap()
        .to_owned();
    let cancel = session
        .begin_action("btw_cancel", &json!({"recordId":id}))
        .await
        .unwrap();
    settle(session.as_mut(), cancel).await;
    assert_eq!(
        session.ui_state().unwrap()["sideQuestions"]["records"][0]["turns"][0]["status"],
        "cancelled"
    );
    let follow = session
        .begin_action("btw", &json!({"recordId":id,"question":"follow up"}))
        .await
        .unwrap();
    settle(session.as_mut(), follow).await;
    wait_for_side_completion(session.as_mut()).await;
    let ui = session.ui_state().unwrap();
    assert_eq!(
        ui["sideQuestions"]["records"][0]["turns"][1]["answer"],
        "Native side answer"
    );
    assert!(ui["sideQuestions"]["runningId"].is_null());
    session.close().await.unwrap();
}

#[tokio::test]
async fn a_side_question_restores_from_native_history_and_empty_btw_opens_that_history() {
    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("hello", &spec.config).await.unwrap();
    complete(session.as_mut()).await;
    command(session.as_mut(), "/btw first question").await;
    wait_for_side_completion(session.as_mut()).await;
    let records = session.ui_state().unwrap()["sideQuestions"]["records"].clone();
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let mut restored = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    assert_eq!(
        restored.ui_state().unwrap()["sideQuestions"]["records"],
        records
    );
    assert!(restored.ui_state().unwrap()["sideQuestions"]["reveal"].is_null());
    command(restored.as_mut(), "/btw").await;
    assert!(restored.ui_state().unwrap()["sideQuestions"]["reveal"].is_string());
    restored.close().await.unwrap();
}

#[tokio::test]
async fn execution_routes_a_blocked_live_command_before_foreground_interruption() {
    let (root, client, _) = fixture();
    std::fs::create_dir_all(root.path().join("agent")).unwrap();
    std::fs::write(root.path().join("agent/side-commands"), "").unwrap();
    let host = crate::test_support::Fixture::new();
    let (execution, _) = crate::test_support::worker_with_client(
        &host,
        model::creation::Creations::default(),
        Box::new(client),
    );
    let created = crate::test_support::request(
        &execution,
        "agent.create.request",
        json!({"config":{"provider":"omp","cwd":host.cwd}}),
    )
    .await;
    let id = &created["agentId"];
    for text in ["wait", "/usage", "/model test/model"] {
        crate::test_support::request(
            &execution,
            "agent.message.send.request",
            json!({"agentId":id,"text":text}),
        )
        .await;
    }
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert!(
        !calls.lines().any(|line| {
            let value: Value = serde_json::from_str(line).unwrap();
            value["type"]
                .as_str()
                .is_some_and(|kind| kind.starts_with("abort"))
        }),
        "queries and blocked host controls must not abort the active model"
    );
    std::fs::write(root.path().join("agent/finish-wait"), "").unwrap();
    crate::test_support::request(
        &execution,
        "agent.finish.wait.request",
        json!({"agentId":id}),
    )
    .await;
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn live_custom_query_names_keep_their_native_owner_and_execute_normally() {
    for (name, input, source) in [
        ("usage", "/usage", "extension"),
        ("advisor", "/advisor status", "custom"),
    ] {
        let (root, client, _) = fixture();
        std::fs::create_dir_all(root.path().join("agent")).unwrap();
        std::fs::write(
            root.path().join("agent/plugins.json"),
            json!([{"name":name,"source":source,"directOutput":"Actual custom owner output"}])
                .to_string(),
        )
        .unwrap();
        let host = crate::test_support::Fixture::new();
        let (execution, _) = crate::test_support::worker_with_client(
            &host,
            model::creation::Creations::default(),
            Box::new(client),
        );
        let created = crate::test_support::request(
            &execution,
            "agent.create.request",
            json!({"config":{"provider":"omp","cwd":host.cwd}}),
        )
        .await;
        let id = &created["agentId"];
        crate::test_support::request(
            &execution,
            "agent.message.send.request",
            json!({"agentId":id,"text":input}),
        )
        .await;
        crate::test_support::request(
            &execution,
            "agent.finish.wait.request",
            json!({"agentId":id}),
        )
        .await;
        let page = crate::test_support::request(
            &execution,
            "agent.timeline.get.request",
            json!({"agentId":id}),
        )
        .await;
        assert!(
            page["entries"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| { entry["item"]["text"] == "Actual custom owner output" }),
            "the registered {source} /{name} must execute instead of being rejected: {page}"
        );
        execution.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn inactive_builtin_query_restores_its_owner_without_creating_a_main_turn() {
    let (root, client, _) = fixture();
    std::fs::create_dir_all(root.path().join("agent")).unwrap();
    std::fs::write(root.path().join("agent/side-commands"), "").unwrap();
    let host = crate::test_support::Fixture::new();
    let (execution, _) = crate::test_support::worker_with_client(
        &host,
        model::creation::Creations::default(),
        Box::new(client.clone()),
    );
    let created = crate::test_support::request(
        &execution,
        "agent.create.request",
        json!({"config":{"provider":"omp","cwd":host.cwd}}),
    )
    .await;
    let id = &created["agentId"];
    execution.shutdown().await.unwrap();
    let (restored, _) = crate::test_support::worker_with_client(
        &host,
        model::creation::Creations::default(),
        Box::new(client.clone()),
    );
    crate::test_support::request(
        &restored,
        "agent.message.send.request",
        json!({"agentId":id,"text":"/usage"}),
    )
    .await;
    let page = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let page = crate::test_support::request(
                &restored,
                "agent.timeline.get.request",
                json!({"agentId":id}),
            )
            .await;
            if page["entries"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["item"]["text"] == "Native usage: 123 tokens")
            {
                break page;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("the builtin result must become visible through the committed read snapshot");
    let entries = page["entries"].as_array().unwrap();
    assert!(
        !entries.iter().any(|entry| {
            matches!(
                entry["item"]["type"].as_str(),
                Some("turn_started" | "user_message")
            )
        }),
        "a restored query must not manufacture a foreground model turn: {page}"
    );
    for _ in 0..2 {
        let repeated = crate::test_support::request(
            &restored,
            "agent.timeline.get.request",
            json!({"agentId":id}),
        )
        .await;
        assert_eq!(repeated["entries"], page["entries"]);
    }
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert!(!calls.lines().any(|line| {
        let call: Value = serde_json::from_str(line).unwrap();
        call["type"] == "prompt" && call["message"] != "/usage"
    }));
    restored.shutdown().await.unwrap();
    let (reopened, _) = crate::test_support::worker_with_client(
        &host,
        model::creation::Creations::default(),
        Box::new(client),
    );
    let recovered = crate::test_support::request(
        &reopened,
        "agent.timeline.get.request",
        json!({"agentId":id}),
    )
    .await;
    assert_eq!(recovered["entries"], page["entries"]);
    reopened.shutdown().await.unwrap();
}
