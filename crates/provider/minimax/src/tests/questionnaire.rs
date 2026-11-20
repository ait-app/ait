use super::*;

#[tokio::test]
async fn ask_user_opens_one_question_per_step_and_returns_native_answers() {
    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("ask-user", &spec.config).await.unwrap();
    let request = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(AgentTurnEvent::PermissionRequested(request)) = session.poll_turn().unwrap()
            {
                break request;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();

    assert_eq!(request["kind"], "question");
    assert_eq!(request["title"], "Project setup");
    assert_eq!(request["metadata"]["toolCallId"], "call-ask");
    let questions = request["input"]["questions"].as_array().unwrap();
    assert_eq!(
        questions
            .iter()
            .map(|question| (&question["header"], &question["allowOther"]))
            .collect::<Vec<_>>(),
        [
            (&json!("Storage"), &json!(true)),
            (&json!("Language"), &json!(true))
        ]
    );
    assert_eq!(
        questions[0]["options"],
        json!([
            {"label":"Postgres","description":"About Postgres"},
            {"label":"SQLite","description":"About SQLite"}])
    );
    assert_eq!(
        session.pending_permissions(),
        std::slice::from_ref(&request)
    );

    session
        .respond_permission(
            request["id"].as_str().unwrap(),
            &json!({"behavior":"allow","updatedInput":{
                "questions":questions,"answers":{"Storage":"Postgres","Language":"Zig"}}}),
        )
        .await
        .unwrap();

    assert!(session.pending_permissions().is_empty());
    assert_eq!(
        finish(session.as_mut()).await.last(),
        Some(&AgentTurnEvent::Completed(Some("Postgres, Zig".into())))
    );
    session.close().await.unwrap();
}
