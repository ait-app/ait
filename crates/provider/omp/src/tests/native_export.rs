use std::io::Read;

use super::*;

#[tokio::test]
async fn exports_native_html_through_the_existing_writer_during_an_active_turn() {
    let (root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("first", &spec.config).await.unwrap();
    complete(session.as_mut()).await;
    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    session.begin_native_html_export().unwrap();
    let mut artifact = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = session.poll_turn().unwrap();
            assert!(!matches!(
                event,
                Some(
                    AgentTurnEvent::Completed(_)
                        | AgentTurnEvent::Cancelled
                        | AgentTurnEvent::Failed
                )
            ));
            if let Some(result) = session.take_native_html_export() {
                break result.unwrap();
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    let mut contents = String::new();
    artifact.file.read_to_string(&mut contents).unwrap();
    assert_eq!(contents, "<html><body>native fixture</body></html>");
    let calls = calls_after(root.path(), 0);
    assert_eq!(count(&calls, "export_html"), 1);
    assert_eq!(count(&calls, "abort"), 0);
    session.cancel_turn(&turn).await.unwrap();
    session.close().await.unwrap();
}
