use super::*;

#[tokio::test]
async fn runtime_inspection_never_blocks_dialogs_before_prompt_admission() {
    let fixtures: Vec<Value> = serde_json::from_str(include_str!(
        "../../../provider/tests/fixtures/omp_dialogs.json"
    ))
    .unwrap();
    for case in fixtures {
        let native = &case["native"];
        let (root, client, spec) = fixture();
        let mut session = client.create_session(&spec).await.unwrap();
        std::fs::write(root.path().join("agent/dialog.json"), native.to_string()).unwrap();
        session
            .start_turn("interactive-fixture", &spec.config)
            .await
            .unwrap();

        let runtime = tokio::time::timeout(Duration::from_secs(2), session.runtime_info())
            .await
            .expect("runtime inspection blocked delivery of an OMP dialog")
            .unwrap();
        assert_eq!(runtime.provider, "omp");
        let request = loop {
            match next(session.as_mut()).await.unwrap() {
                AgentTurnEvent::PermissionRequested(request) => break request,
                AgentTurnEvent::Failed | AgentTurnEvent::Completed(_) => {
                    panic!("dialog ended without user input: {native}");
                }
                _ => {}
            }
        };
        assert_eq!(request, case["request"]);
        assert_eq!(session.pending_permissions(), vec![request.clone()]);
        session
            .respond_permission(request["id"].as_str().unwrap(), &case["response"])
            .await
            .unwrap();
        let events = complete(session.as_mut()).await;
        assert!(events.iter().any(|event| matches!(event,
            AgentTurnEvent::PermissionResolved(id) if *id == request["id"])));
        assert!(
            matches!(events.last(), Some(AgentTurnEvent::Completed(_))),
            "{events:?}"
        );
        assert_eq!(session.pending_permissions(), Vec::<Value>::new());
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn manager_publishes_dialog_attention_and_reconnect_state_before_native_acknowledgement() {
    use domain::agent_runtime::{AgentAttentionReason, registry::AgentRuntimeRegistry};
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
    use provider::service::agent_manager::AgentManager;
    use provider::testing::Manager;
    let (root, client, spec) = fixture();
    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    let mut manager = Manager::new(AgentManager::new(Box::new(registry.clone())));
    manager.register(Box::new(client)).unwrap();
    manager.create("agent", &spec).await.unwrap();
    std::fs::write(
        root.path().join("agent/dialog.json"),
        json!({"method":"select","title":"Write approval","options":["Approve","Deny"]})
            .to_string(),
    )
    .unwrap();

    tokio::time::timeout(
        Duration::from_secs(2),
        manager.send("agent", "interactive-fixture"),
    )
    .await
    .expect("host submission blocked dialog polling")
    .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            manager.poll().await.unwrap();
            let record = registry.get("agent").unwrap().unwrap();
            if record.attention_reason == Some(AgentAttentionReason::Permission) {
                let mut snapshot = json!({});
                manager.control_snapshot(&record, &mut snapshot);
                assert_eq!(
                    snapshot["pendingPermissions"][0]["input"]["questions"][0]["question"],
                    "Write approval"
                );
                assert!(record.last_error.is_none());
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn steering_cannot_hide_a_dialog_while_native_admission_waits_for_its_answer() {
    let (root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    std::fs::write(
        root.path().join("agent/dialog.json"),
        json!({"method":"input","title":"Additional instructions"}).to_string(),
    )
    .unwrap();
    let turn = session
        .start_turn("interactive-fixture", &spec.config)
        .await
        .unwrap();
    let request = loop {
        if let AgentTurnEvent::PermissionRequested(request) = next(session.as_mut()).await.unwrap()
        {
            break request;
        }
    };

    tokio::time::timeout(
        Duration::from_secs(2),
        session.steer_turn(&turn, "Continue after answering"),
    )
    .await
    .expect("steering blocked the pending dialog")
    .unwrap();
    assert_eq!(session.pending_permissions(), vec![request.clone()]);
    session
        .respond_permission(
            request["id"].as_str().unwrap(),
            &json!({"behavior":"allow",
        "updatedInput":{"answers":{"Additional instructions":"Answer"}}}),
        )
        .await
        .unwrap();

    let events = complete(session.as_mut()).await;
    assert!(
        matches!(events.last(), Some(AgentTurnEvent::Completed(_))),
        "{events:?}"
    );
    session.close().await.unwrap();
}

/// Wait for the next host request, failing if the turn ends first.
async fn requested(session: &mut dyn AgentSession) -> Value {
    loop {
        match next(session).await.unwrap() {
            AgentTurnEvent::PermissionRequested(request) => return request,
            event @ (AgentTurnEvent::Completed(_)
            | AgentTurnEvent::Cancelled
            | AgentTurnEvent::Failed) => panic!("turn ended without a request: {event:?}"),
            _ => {}
        }
    }
}

/// The `extension_ui_response` values the fixture received.
fn native_answers(root: &std::path::Path) -> Vec<Value> {
    calls_after(root, 0)
        .into_iter()
        .filter(|call| call["type"] == "extension_ui_response")
        .map(|mut call| {
            call.as_object_mut().unwrap().remove("id");
            call.as_object_mut().unwrap().remove("type");
            call
        })
        .collect()
}

#[tokio::test]
async fn tool_approvals_are_tool_permissions_answered_with_the_chosen_native_option() {
    let (root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("approval", &spec.config).await.unwrap();

    let request = requested(session.as_mut()).await;

    assert_eq!(request["kind"], "tool");
    assert_eq!(request["title"], "Allow tool: bash");
    assert_eq!(
        request["detail"],
        json!({"type":"shell","command":"printf 'ok'\necho done"})
    );
    assert_eq!(session.pending_permissions(), vec![request.clone()]);
    let session_scope = request["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action["label"] == "Approve bash for this session")
        .unwrap();
    session
        .respond_permission(
            request["id"].as_str().unwrap(),
            &json!({"behavior":"allow","selectedActionId":session_scope["id"]}),
        )
        .await
        .unwrap();
    let events = complete(session.as_mut()).await;
    assert!(
        matches!(events.last(), Some(AgentTurnEvent::Completed(_))),
        "{events:?}"
    );
    assert!(events.iter().any(|event| matches!(event,
        AgentTurnEvent::Timeline(entry) if entry.item["type"] == "tool_call"
            && entry.item["status"] == "completed")));
    session.close().await.unwrap();
    assert_eq!(
        native_answers(root.path()),
        vec![json!({"value":"Approve bash for this session"})]
    );
}

#[tokio::test]
async fn html_ask_fields_are_one_card_each_and_replay_omp_follow_ups_without_the_ask_dialog() {
    let (root, client, spec) = fixture_with(&[("OMP_FIXTURE_REJECT_ASK_DIALOG", "1")]);
    // A refused set_ask_dialog only loses the structured fast path.
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("html-ask", &spec.config).await.unwrap();

    let database = requested(session.as_mut()).await;
    assert_eq!(database["name"], "ask");
    assert_eq!(
        database["input"]["questions"][0],
        json!({"header":"Which database?","question":"Which database?","answerKey":"value",
            "options":[{"label":"A,B"},{"label":"C"}],"multiSelect":false,"allowOther":true})
    );
    session
        .respond_permission(
            database["id"].as_str().unwrap(),
            &json!({"behavior":"allow","updatedInput":{"answers":{"Which database?":"Custom DB"}}}),
        )
        .await
        .unwrap();
    let features = requested(session.as_mut()).await;
    assert_eq!(features["input"]["questions"][0]["multiSelect"], true);
    session
        .respond_permission(
            features["id"].as_str().unwrap(),
            &json!({"behavior":"allow","updatedInput":{"answers":{
                "value":["Search","Auth","Custom feat"]}}}),
        )
        .await
        .unwrap();
    let events = complete(session.as_mut()).await;

    assert!(
        matches!(events.last(), Some(AgentTurnEvent::Completed(_))),
        "{events:?}"
    );
    assert_eq!(session.pending_permissions(), Vec::<Value>::new());
    let result = events
        .iter()
        .find_map(|event| match event {
            // D10: Paseo shows an answered ask as a labelled text row.
            AgentTurnEvent::Timeline(entry) if entry.item["name"] == "ask" => {
                entry.item["detail"]["text"].as_str()
            }
            _ => None,
        })
        .unwrap()
        .to_owned();
    for answer in ["Custom DB", "Search", "Auth", "Custom feat"] {
        assert!(result.contains(answer), "{result}");
    }
    session.close().await.unwrap();
    assert_eq!(
        native_answers(root.path()),
        [
            "Other (type your own)",
            "Custom DB",
            "Search",
            "Auth",
            "Other (type your own)",
            "Custom feat",
            "\u{2714} Done selecting",
        ]
        .map(|value| json!({"value":value}))
    );
}

#[tokio::test]
async fn a_required_checkbox_group_shown_as_one_choice_finishes_when_omp_reveals_done() {
    let (root, client, spec) = fixture_with(&[("OMP_FIXTURE_ASK_REQUIRED", "1")]);
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("html-ask", &spec.config).await.unwrap();
    let database = requested(session.as_mut()).await;
    session
        .respond_permission(
            database["id"].as_str().unwrap(),
            &json!({"behavior":"allow","updatedInput":{"answers":{"value":"C"}}}),
        )
        .await
        .unwrap();

    // Without Done or a count, OMP's first prompt looks like a single choice.
    let features = requested(session.as_mut()).await;
    assert_eq!(features["input"]["questions"][0]["multiSelect"], false);
    session
        .respond_permission(
            features["id"].as_str().unwrap(),
            &json!({"behavior":"allow","updatedInput":{"answers":{"value":"Search"}}}),
        )
        .await
        .unwrap();
    let events = complete(session.as_mut()).await;

    assert!(
        matches!(events.last(), Some(AgentTurnEvent::Completed(_))),
        "{events:?}"
    );
    session.close().await.unwrap();
    assert_eq!(
        native_answers(root.path()),
        ["C", "Search", "\u{2714} Done selecting"].map(|value| json!({"value":value}))
    );
}

#[tokio::test]
async fn denying_an_ask_card_cancels_omp_ask_and_clears_the_replay() {
    let (root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("html-ask", &spec.config).await.unwrap();
    let database = requested(session.as_mut()).await;

    session
        .respond_permission(
            database["id"].as_str().unwrap(),
            &json!({"behavior":"deny"}),
        )
        .await
        .unwrap();
    let events = complete(session.as_mut()).await;

    assert!(
        events.iter().any(|event| matches!(event,
            AgentTurnEvent::Timeline(entry) if entry.item["name"] == "ask"
                && entry.item["status"] == "failed")),
        "{events:?}"
    );
    assert_eq!(session.pending_permissions(), Vec::<Value>::new());
    session.close().await.unwrap();
    assert_eq!(native_answers(root.path()), vec![json!({"cancelled":true})]);
}

#[tokio::test]
async fn open_url_is_an_assistant_message_with_the_link_and_no_reply() {
    let (root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("open-url", &spec.config).await.unwrap();

    let events = complete(session.as_mut()).await;

    let text = events
        .iter()
        .find_map(|event| match event {
            AgentTurnEvent::Timeline(entry)
                if entry.item["metadata"]["nativeType"] == "open_url" =>
            {
                assert_eq!(entry.item["type"], "assistant_message");
                entry.item["text"].as_str()
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(
        text,
        "[Open URL](https://example.com/device)\nURL: https://example.com/device\n\
         Launch URL: https://example.com/device?code=ABCD\n\nEnter code ABCD"
    );
    assert!(
        matches!(events.last(), Some(AgentTurnEvent::Completed(Some(reply))) if reply == "Hello world")
    );
    session.close().await.unwrap();
    assert!(native_answers(root.path()).is_empty());
}
