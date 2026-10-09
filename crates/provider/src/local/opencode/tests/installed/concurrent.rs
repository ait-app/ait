use super::*;

#[tokio::test]
#[ignore = "requires AIT_TEST_OPENCODE_BIN; isolated config and loopback model only"]
async fn installed_opencode_keeps_sessions_writable_after_sibling_and_history_helpers_close() {
    let (_root, client, spec, _server) =
        installed_fixture(Router::new().route("/v1/chat/completions", post(answer))).await;
    let mut first = client.create_session(&spec).await.unwrap();
    first.start_turn("first input", &spec.config).await.unwrap();
    assert_completed(first.as_mut()).await;
    let handle = first.persistence().unwrap();

    let mut second = client.create_session(&spec).await.unwrap();
    second
        .start_turn("sibling input", &spec.config)
        .await
        .unwrap();
    assert_completed(second.as_mut()).await;
    assert_eq!(client.history(&handle, &spec.cwd).await.unwrap().len(), 2);
    second.close().await.unwrap();

    first
        .start_turn("after sibling closes", &spec.config)
        .await
        .unwrap();
    assert_completed(first.as_mut()).await;
    first.close().await.unwrap();
    assert_eq!(client.history(&handle, &spec.cwd).await.unwrap().len(), 4);
}
