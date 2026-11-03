use super::*;

#[tokio::test]
async fn early_ack_profile_rejects_custom_commands_before_admission_and_preserves_normal_prompts() {
    let (root, mut client, spec) = fixture("command-catalog");
    client
        .environment
        .insert("AIT_ACP_VERSION".into(), "2.1.3".into());
    let mut session = client.create_session(&spec).await.unwrap();
    assert!(session.refresh_commands().await.unwrap().is_some());
    for text in [
        "/native:review subject",
        "  /native:review subject",
        "/unknown",
    ] {
        assert_eq!(
            session.start_turn(text, &spec.config).await,
            Err(AgentSessionError::Rejected)
        );
        assert!(
            session
                .failure_message()
                .unwrap()
                .contains("lifecycle receipts")
        );
    }
    assert!(
        !requests(&root)
            .iter()
            .any(|request| request["method"] == "session/prompt")
    );
    session
        .start_turn("Native catalog changes", &spec.config)
        .await
        .unwrap();
    completed(session.as_mut()).await;
    assert!(session.failure_message().is_none());
    session.close().await.unwrap();
}

#[tokio::test]
async fn commands_use_the_owned_acp_session_and_updates_survive_the_foreground_reader() {
    let (root, client, spec) = fixture("command-catalog");
    let mut session = client.create_session(&spec).await.unwrap();
    let catalog = session.refresh_commands().await.unwrap().unwrap();
    assert_eq!(catalog[0]["name"], "native:review");
    assert_eq!(catalog[0]["argumentHint"], "<topic>");
    session.refresh_commands().await.unwrap();
    assert_eq!(
        requests(&root)
            .iter()
            .filter(|request| request["method"] == "session/new")
            .count(),
        1
    );
    assert!(
        !requests(&root)
            .iter()
            .any(|request| request["method"] == "session/prompt")
    );
    session
        .start_turn("Native catalog changes", &spec.config)
        .await
        .unwrap();
    completed(session.as_mut()).await;
    assert_eq!(session.refresh_commands().await.unwrap(), Some(Vec::new()));
    session.close().await.unwrap();
}

#[tokio::test]
async fn draft_catalog_discovery_removes_only_its_owned_temporary_session_without_inference() {
    let (root, client, spec) = fixture("command-catalog");
    let catalog = client.commands(&spec).await.unwrap();
    assert_eq!(catalog[0]["name"], "native:review");
    let log = requests(&root);
    assert_eq!(
        log.iter()
            .filter(|request| request["method"] == "session/new")
            .count(),
        1
    );
    assert_eq!(
        log.iter()
            .filter(|request| request["method"] == "session/delete")
            .count(),
        1
    );
    assert!(
        !log.iter()
            .any(|request| request["method"] == "session/prompt")
    );
}

#[tokio::test]
async fn draft_discovery_without_native_deletion_does_not_create_a_persistent_session() {
    let (root, client, spec) = fixture("no-delete");
    assert!(matches!(
        client.commands(&spec).await,
        Err(AgentSessionError::Unavailable)
    ));
    assert!(
        !requests(&root)
            .iter()
            .any(|request| request["method"] == "session/new")
    );
}
