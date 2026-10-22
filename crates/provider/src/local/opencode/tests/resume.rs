use super::*;

#[tokio::test]
async fn inspected_resume_metadata_preserves_client_correlations() {
    let (root, client, spec) = fixture("normal");
    std::fs::write(
        root.path().join("native-fixture.json"),
        json!({"seq":1,"history":[
            {"id":"user1","type":"user","text":"first prompt"},
            {"id":"answer1","type":"assistant",
                "content":[{"type":"text","text":"authoritative answer"}]}
        ]})
        .to_string(),
    )
    .unwrap();
    let clients = json!({"user1":"client-message-1"});
    let mut handle = domain::agent_runtime::AgentPersistenceHandle {
        provider: spec.provider.clone(),
        session_id: "ses_one".into(),
        native_handle: None,
        metadata: Some(BTreeMap::from([
            ("cwd".into(), json!(spec.cwd)),
            ("opencode".into(), json!({"clients":clients})),
        ])),
    };

    let history = client.inspect_session(&handle, &spec.cwd).await.unwrap();
    assert_eq!(
        history.entries[0].item["clientMessageId"],
        "client-message-1"
    );
    assert_eq!(history.resume_metadata["opencode"]["clients"], clients);
    handle.metadata = Some(history.resume_metadata);
    let mut session = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();

    assert_eq!(
        session.persistence().unwrap().metadata.unwrap()["opencode"]["clients"],
        clients
    );
    session.close().await.unwrap();
    assert!(
        requests(&root)
            .iter()
            .all(|request| request["method"] != "session/prompt")
    );
}

#[tokio::test]
async fn moved_native_session_settles_resumes_and_inspects_at_its_new_directory() {
    let (root, client, spec) = fixture("normal");
    let moved = root.path().join("worktree").to_string_lossy().into_owned();
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_turn("Move this session", &spec.config)
        .await
        .unwrap();
    completed(session.as_mut()).await;
    session
        .start_turn("after move", &spec.config)
        .await
        .unwrap();
    completed(session.as_mut()).await;
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();

    let mut session = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    session
        .start_turn("after resume", &spec.config)
        .await
        .unwrap();
    completed(session.as_mut()).await;
    assert!(session.failure_diagnostic().is_none());
    session.close().await.unwrap();

    let history = client.inspect_session(&handle, &spec.cwd).await.unwrap();
    assert_eq!(history.descriptor.cwd, moved);
    assert!(
        history
            .entries
            .iter()
            .any(|entry| entry.item["text"] == "after resume")
    );
    assert!(requests(&root).iter().any(|request| {
        request["method"] == "session/load" && request["params"]["cwd"] == moved
    }));
}
