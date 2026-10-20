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
