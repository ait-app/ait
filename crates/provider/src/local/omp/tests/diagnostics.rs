use super::*;

#[tokio::test]
async fn native_errors_retain_detail_after_close_and_successful_retries_clear_it() {
    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_turn("native-error-recovered", &spec.config)
        .await
        .unwrap();
    assert!(matches!(
        complete(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    assert_eq!(session.failure_message(), None);
    assert_eq!(session.failure_diagnostic(), None);
    session
        .start_turn("native-error", &spec.config)
        .await
        .unwrap();
    assert_eq!(
        complete(session.as_mut()).await.last(),
        Some(&AgentTurnEvent::Failed)
    );
    assert_eq!(
        session.failure_diagnostic().unwrap().category,
        crate::ports::diagnostics::Category::Quota
    );
    let message = session.failure_message().unwrap().into_owned();
    assert!(message.contains("Oh My Pi"));
    assert!(message.contains("insufficient balance"));
    assert!(!message.contains("fixture-secret"));
    session.close().await.unwrap();
    assert_eq!(session.failure_message().as_deref(), Some(message.as_str()));
}
