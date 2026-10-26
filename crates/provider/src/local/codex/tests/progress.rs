use super::*;

mod command_actions;

#[tokio::test]
async fn compaction_terminal_fallback_closes_loading_and_releases_manual_admission() {
    for mode in ["compaction-without-final-item", "legacy-compaction"] {
        let fixture = Fixture::new();
        fixture.mode(mode);
        let client = fixture.client();
        let spec = fixture.spec();
        let mut session = client.create_session(&spec).await.unwrap();
        if mode == "legacy-compaction" {
            let mut prompt = crate::protocol::prompt::AgentPrompt::text("/compact");
            prompt.client_message_id = Some("manual-legacy".into());
            session.out_of_band(&prompt).await.unwrap();
        } else {
            session
                .start_turn("auto-compaction", &spec.config)
                .await
                .unwrap();
        }
        let statuses = tokio::time::timeout(Duration::from_secs(2), async {
            let mut statuses = Vec::new();
            loop {
                match session.poll_turn().unwrap() {
                    Some(AgentTurnEvent::Timeline(entry)) if entry.item["type"] == "compaction" => {
                        statuses.push(entry.item["status"].as_str().unwrap().to_owned());
                        if entry.item["status"] == "completed" {
                            break statuses;
                        }
                    }
                    Some(AgentTurnEvent::Completed(_)) => break statuses,
                    Some(AgentTurnEvent::Failed) => panic!("compaction failed"),
                    _ => tokio::task::yield_now().await,
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(statuses.last().map(String::as_str), Some("completed"));
        assert!(!session.pending_foreground());
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn native_terminal_input_is_displayed_with_its_process_and_command() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let spec = fixture.spec();
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_turn("terminal-input", &spec.config)
        .await
        .unwrap();
    let entries = tokio::time::timeout(Duration::from_secs(3), async {
        let mut entries = Vec::new();
        loop {
            match session.poll_turn().unwrap() {
                Some(AgentTurnEvent::Timeline(entry)) if entry.item["name"] == "terminal" => {
                    entries.push(entry);
                }
                Some(AgentTurnEvent::Completed(_)) => break entries,
                Some(AgentTurnEvent::Failed) => panic!("terminal input failed"),
                None => tokio::task::yield_now().await,
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].item["detail"]["text"], "confirm\n");
    assert_eq!(entries[0].item["detail"]["label"], "interactive");
    assert_eq!(entries[0].item["metadata"]["processId"], "42");
    session.close().await.unwrap();
}

#[tokio::test]
async fn native_compaction_emits_loading_before_completion_for_auto_and_manual_requests() {
    let fixture = Fixture::new();
    fixture.mode("workflows");
    let client = fixture.client();
    for (manual, busy) in [(false, false), (true, false), (true, true)] {
        let spec = fixture.spec();
        let mut session = client.create_session(&spec).await.unwrap();
        if busy {
            session.start_turn("hang", &spec.config).await.unwrap();
        }
        if manual {
            let mut prompt = crate::protocol::prompt::AgentPrompt::text("/compact");
            prompt.client_message_id = Some(format!("compact-{busy}"));
            session.out_of_band(&prompt).await.unwrap();
        } else {
            session
                .start_turn("auto-compaction", &spec.config)
                .await
                .unwrap();
        }
        let mut statuses = Vec::new();
        tokio::time::timeout(Duration::from_secs(3), async {
            while statuses.len() < 2 {
                match session.poll_turn().unwrap() {
                    Some(AgentTurnEvent::Timeline(entry)) if entry.item["type"] == "compaction" => {
                        statuses.push(entry.item["status"].as_str().unwrap().to_owned());
                    }
                    Some(AgentTurnEvent::Failed) => panic!("native compaction failed"),
                    _ => tokio::time::sleep(Duration::from_millis(5)).await,
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(statuses, ["loading", "completed"]);
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn native_plan_progress_is_a_durable_todo_snapshot_or_a_reviewable_plan() {
    let fixture = Fixture::new();
    fixture.mode("workflows");
    let client = fixture.client();
    let mut spec = fixture.spec();
    for planning in [false, true] {
        spec.config.feature_values = Some(std::collections::BTreeMap::from([(
            "plan_mode".to_owned(),
            json!(planning),
        )]));
        let mut session = client.create_session(&spec).await.unwrap();
        session
            .start_turn("plan-progress", &spec.config)
            .await
            .unwrap();
        let mut todos = Vec::new();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                match session.poll_turn().unwrap() {
                    Some(AgentTurnEvent::Timeline(entry)) if entry.item["type"] == "todo" => {
                        todos.push(entry);
                    }
                    Some(AgentTurnEvent::Completed(_)) => break,
                    Some(AgentTurnEvent::Failed) => panic!("plan progress failed"),
                    _ => tokio::time::sleep(Duration::from_millis(5)).await,
                }
            }
        })
        .await
        .unwrap();
        let handle = session.persistence().unwrap();
        if planning {
            assert_eq!(todos, [] as [crate::protocol::timeline::NativeItem; 0]);
            assert!(
                session.pending_permissions()[0]["input"]["plan"]
                    .as_str()
                    .unwrap()
                    .contains("Second step")
            );
        } else {
            assert_eq!(todos[0].item["items"][1]["status"], "in_progress");
            assert_eq!(todos[0].item["items"][0]["completed"], true);
        }
        session.close().await.unwrap();
        if !planning {
            assert!(
                client
                    .history(&handle, &spec.cwd)
                    .await
                    .unwrap()
                    .iter()
                    .any(|entry| entry.item["type"] == "todo")
            );
        }
    }
}
