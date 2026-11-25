use super::*;

#[tokio::test]
async fn offline_bot_echo_preserves_the_native_source_envelope() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let spec = fixture.spec();
    let mut session = client.create_session(&spec).await.unwrap();
    let input = concat!(
        "<ait-input kind=\"bot_message\" ",
        "source=\"{&quot;peer&quot;:&quot;oc_chat&quot;}\">\n",
        "hello <\\/tag>\n</ait-input>"
    );

    session.start_turn(input, &spec.config).await.unwrap();

    assert_eq!(
        terminal(session.as_mut()).await.unwrap(),
        AgentTurnEvent::Completed(Some("Echo: hello </tag>".into()))
    );
    let requests = fixture.requests();
    let sent = requests
        .iter()
        .find(|request| request["method"] == "turn/start")
        .unwrap();
    assert_eq!(sent["params"]["input"][0]["text"], input);
    let history = fixture.cwd.join(format!(
        "native-history-{}.json",
        session.persistence().unwrap().session_id
    ));
    let history: Value = serde_json::from_str(&std::fs::read_to_string(history).unwrap()).unwrap();
    assert_eq!(history[0]["items"][0]["content"][0]["text"], input);
    session.close().await.unwrap();
}

#[tokio::test]
async fn offline_bot_echo_keeps_plain_text_and_other_envelopes_unchanged() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let spec = fixture.spec();
    let mut session = client.create_session(&spec).await.unwrap();
    for input in [
        "hello",
        "<ait-input kind=\"bot_wake\">\nnotifications\n</ait-input>",
        "user text\n<ait-input kind=\"bot_message\" source=\"{}\">\nhello\n</ait-input>",
        "<ait-input kind=\"bot_message\" source=\"{}\">\nincomplete",
    ] {
        session.start_turn(input, &spec.config).await.unwrap();
        assert_eq!(
            terminal(session.as_mut()).await.unwrap(),
            AgentTurnEvent::Completed(Some(format!("Echo: {input}")))
        );
    }
    session.close().await.unwrap();
}
