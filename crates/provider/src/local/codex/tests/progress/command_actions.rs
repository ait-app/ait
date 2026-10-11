use super::*;

#[tokio::test]
async fn parsed_actions_survive_native_stdio_progress_completion_and_history_reload() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let spec = fixture.spec();
    let mut session = client.create_session(&spec).await.unwrap();
    let handle = session.persistence().unwrap();
    session
        .start_turn("command-actions", &spec.config)
        .await
        .unwrap();
    let mut progress = Vec::new();
    let mut completed = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match session.poll_turn().unwrap() {
                Some(AgentTurnEvent::Progress { entry, .. }) => progress.push(entry),
                Some(AgentTurnEvent::Timeline(entry)) if entry.item["type"] == "tool_call" => {
                    completed.push(entry);
                }
                Some(AgentTurnEvent::Completed(_)) => break,
                Some(AgentTurnEvent::Failed) => panic!("command action turn failed"),
                None => tokio::time::sleep(Duration::from_millis(5)).await,
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    session.close().await.unwrap();

    assert_eq!(progress.len(), 4);
    assert_eq!(completed.len(), 3);
    assert_eq!(completed[0].item["detail"]["type"], "read");
    assert_eq!(completed[0].item["detail"]["filePath"], "file");
    assert_eq!(completed[1].item["detail"]["command"], "echo done");
    for (index, entry) in completed.iter().enumerate() {
        let call_id = if index == 2 {
            "command:output".to_owned()
        } else {
            format!("command:{index}")
        };
        assert_eq!(entry.item["callId"], call_id);
        assert_eq!(entry.item["status"], "completed");
        assert_eq!(progress[index].key, entry.key);
        assert_eq!(progress[index].item["status"], "running");
        let field = if index == 0 { "content" } else { "output" };
        assert_eq!(progress[index].item["detail"][field], "");
        assert_eq!(
            entry.item["detail"][field],
            if index == 2 { "offline output" } else { "" }
        );
    }
    assert_eq!(progress[3].key, completed[2].key);
    assert_eq!(progress[3].item["detail"]["output"], "offline output");
    let history = client.history(&handle, &spec.cwd).await.unwrap();
    let tools: Vec<_> = history
        .into_iter()
        .filter(|entry| entry.item["type"] == "tool_call")
        .collect();
    assert_eq!(tools.len(), completed.len());
    for (stored, live) in tools.iter().zip(&completed) {
        assert_eq!(stored.key, live.key);
        assert_eq!(stored.item, live.item);
    }
}
