use super::*;
use std::collections::BTreeMap;

#[tokio::test]
async fn history_and_inspection_restore_answers_inside_the_original_turn() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let question = json!({"type":"agentMessage","id":"question","delivery":"async",
        "questions":[{"title":"Which runtime?"}]});
    let anchor = json!({"type":"agentMessage","id":"working","text":"Still working"});
    let original = json!([
        {"id":"t1","status":"completed","items":[question,anchor,
            {"id":"done","type":"agentMessage","text":"All done"}]},
        {"id":"t2","status":"completed","items":[
            {"id":"later","type":"userMessage","content":[{"type":"text","text":"Next task"}]}]}
    ]);
    std::fs::write(
        fixture.cwd.join("native-history-source.json"),
        original.to_string(),
    )
    .unwrap();
    let mut questions = async_questions::Questions::default();
    questions.receive(&question).unwrap();
    questions.observe(
        &discovery::timeline_item(&anchor, "t1", "2026-01-01T00:00:00Z")
            .unwrap()
            .unwrap(),
    );
    let answer = questions
        .resolve(
            "permission-question",
            &json!({"behavior":"allow","updatedInput":{"answers":{"Question 1":"Rust"}}}),
        )
        .unwrap();
    let handle = AgentPersistenceHandle {
        provider: "codex".into(),
        session_id: "source".into(),
        native_handle: None,
        metadata: Some(BTreeMap::from([(
            "asyncQuestions".into(),
            questions.saved().unwrap(),
        )])),
    };

    let history = client.history(&handle, &fixture.spec().cwd).await.unwrap();
    let inspected = client
        .inspect_session(&handle, &fixture.spec().cwd)
        .await
        .unwrap();

    assert_eq!(history[2], answer);
    assert_eq!(history[3].key, "native:t1:done");
    assert_eq!(history[4].key, "native:t2:later");
    assert_eq!(inspected.entries, history);
    assert_eq!(
        client.history(&handle, &fixture.spec().cwd).await.unwrap(),
        history
    );
}

#[tokio::test]
async fn inspect_and_rewind_preserve_only_controls_belonging_to_retained_history() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let question = json!({"type":"agentMessage","id":"question","delivery":"async","questions":[{"title":"Which runtime?"}]});
    let original = json!([
        {"id":"t1","status":"completed","items":[{"id":"u1","type":"userMessage","content":[{"type":"text","text":"first"}]},question,{"id":"plan","type":"plan","text":"Implement tests"}]},
        {"id":"t2","status":"completed","items":[{"id":"u2","type":"userMessage","content":[{"type":"text","text":"second"}]}]}
    ]);
    let path = fixture.cwd.join("native-history-source.json");
    std::fs::write(&path, original.to_string()).unwrap();
    let mut questions = async_questions::Questions::default();
    questions.receive(&question).unwrap();
    let plan = plans::Plan::receive(&json!({"id":"plan","text":"Implement tests"}), "t1")
        .unwrap()
        .unwrap();
    let note = crate::protocol::timeline::NativeItem {
        key: "native:control-note".into(),
        turn_id: Some("t1".into()),
        timestamp: "2026-01-01T00:00:00Z".into(),
        item: json!({"type":"notification","message":"Control completed","level":"info"}),
    };
    let handle = AgentPersistenceHandle {
        provider: "codex".into(),
        session_id: "source".into(),
        native_handle: None,
        metadata: Some(BTreeMap::from([
            ("pendingPlan".into(), json!(plan)),
            ("controlNotes".into(), json!([note])),
            ("asyncQuestions".into(), questions.saved().unwrap()),
        ])),
    };
    let inspected = client
        .inspect_session(&handle, &fixture.spec().cwd)
        .await
        .unwrap();
    assert_eq!(inspected.resume_metadata["pendingPlan"], json!(plan));
    assert_eq!(inspected.resume_metadata["controlNotes"], json!([note]));
    assert_eq!(
        inspected.resume_metadata["asyncQuestions"],
        questions.saved().unwrap()
    );
    assert_eq!(
        inspected
            .entries
            .iter()
            .filter(|entry| entry.key == note.key)
            .count(),
        1
    );
    let history = client.history(&handle, &fixture.spec().cwd).await.unwrap();
    assert_eq!(
        history.iter().filter(|entry| entry.key == note.key).count(),
        1
    );
    let retained = client.rewind(&handle, &fixture.spec(), "u2").await.unwrap();
    for key in ["pendingPlan", "controlNotes", "asyncQuestions"] {
        assert_eq!(
            retained.resume_metadata.get(key),
            inspected.resume_metadata.get(key),
            "{key}"
        );
    }
    let empty = client.rewind(&handle, &fixture.spec(), "u1").await.unwrap();
    for key in ["pendingPlan", "controlNotes", "asyncQuestions"] {
        assert!(!empty.resume_metadata.contains_key(key), "{key}");
    }
    assert_eq!(std::fs::read_to_string(path).unwrap(), original.to_string());
}

#[tokio::test]
async fn foreign_handles_and_unavailable_binaries_are_rejected_before_native_work() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let handle = AgentPersistenceHandle {
        provider: "claude".into(),
        session_id: "source".into(),
        native_handle: None,
        metadata: None,
    };
    assert!(matches!(
        client.history(&handle, &fixture.spec().cwd).await,
        Err(AgentSessionError::Unavailable)
    ));
    assert!(matches!(
        client.inspect_session(&handle, &fixture.spec().cwd).await,
        Err(AgentSessionError::Unavailable)
    ));
    assert!(matches!(
        client.rewind(&handle, &fixture.spec(), "u1").await,
        Err(AgentSessionError::Unavailable)
    ));
    let unavailable = CodexClient::new(fixture.root.path().join("absent"));
    let mut spec = fixture.spec();
    spec.config.model = Some("model".into());
    assert!(matches!(
        unavailable.draft_features(&spec).await,
        Err(AgentSessionError::Unavailable)
    ));
    client
        .validate_config(&StoredAgentConfig::default())
        .unwrap();
    assert!(!fixture.cwd.join("native-requests.jsonl").exists());
}
