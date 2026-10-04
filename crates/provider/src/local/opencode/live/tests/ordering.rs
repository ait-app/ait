use super::*;
use crate::storage::timeline::Timeline;

fn stream_events(version: Version, number: usize) -> Vec<Value> {
    match version {
        Version::V1 => {
            let message = format!("msg_0123456789abABCDEFGHIJKLM{number}");
            vec![
                json!({"type":"message.updated","properties":{"info":{"id":message,"sessionID":"ses_one","role":"assistant"}}}),
                json!({"type":"message.part.updated","properties":{"part":{"id":format!("prt_0123456789abABCDEFGHIJKLM{number}"),"messageID":message,"sessionID":"ses_one","type":"text","text":"ans"}}}),
            ]
        }
        Version::V2 => vec![
            json!({"type":"session.text.delta","data":{"sessionID":"ses_one","assistantMessageID":format!("answer{number}"),"delta":"ans"}}),
        ],
    }
}

fn persist(timeline: &Timeline, event: AgentTurnEvent) {
    match event {
        AgentTurnEvent::Timeline(entry) => {
            timeline.append("agent", "opencode", &[entry]).unwrap();
        }
        AgentTurnEvent::Progress { observation, entry } => {
            timeline
                .progress("agent", "opencode", &observation, &entry)
                .unwrap();
        }
        AgentTurnEvent::Failed => panic!("streaming failed"),
        _ => {}
    }
}

#[tokio::test]
async fn both_protocols_keep_users_before_streamed_replies_across_turns_and_resume() {
    for version in [Version::V1, Version::V2] {
        let fixture = Fixture::start(version).await;
        let client = OpenCodeClient::new(fixture.binary.clone());
        let spec = spec(&fixture);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("timeline.sqlite");
        let mut session = client.create_session(&spec).await.unwrap();
        for number in 1..=2 {
            let timeline = Timeline::open(&path).unwrap();
            {
                let mut state = fixture.state.lock().unwrap();
                state.busy = true;
                state.unfinished_while_busy = true;
                state.stream_events = stream_events(version, number);
            }
            let prompt = AgentPrompt {
                text: format!("question {number}"),
                client_message_id: Some(format!("client-{number}")),
                ..Default::default()
            };
            session.start_input(&prompt, &spec.config).await.unwrap();
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    if let Some(event) = session.poll_turn().unwrap() {
                        let progress = matches!(event, AgentTurnEvent::Progress { .. });
                        persist(&timeline, event);
                        if progress {
                            break;
                        }
                    } else {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                }
            })
            .await
            .unwrap();
            let rows = timeline.read("agent").unwrap().1;
            let user = rows
                .iter()
                .position(|row| row.entry.item["clientMessageId"] == format!("client-{number}"))
                .unwrap();
            assert_eq!(rows[user].entry.item["text"], prompt.text);
            assert!(user < rows.len() - 1);
            assert_eq!(rows.last().unwrap().entry.item["type"], "assistant_message");
            fixture.state.lock().unwrap().busy = false;
            let finished = drain(session.as_mut()).await;
            assert!(matches!(
                finished.last(),
                Some(AgentTurnEvent::Completed(_))
            ));
            for event in finished {
                persist(&timeline, event);
            }
            let handle = session.persistence().unwrap();
            session.close().await.unwrap();
            let before = timeline.read("agent").unwrap().1;
            drop(timeline);
            let reopened = Timeline::open(&path).unwrap();
            assert_eq!(
                reopened
                    .read("agent")
                    .unwrap()
                    .1
                    .iter()
                    .map(crate::storage::timeline::Row::value)
                    .collect::<Vec<_>>(),
                before
                    .iter()
                    .map(crate::storage::timeline::Row::value)
                    .collect::<Vec<_>>()
            );
            let history = client.history(&handle, &spec.cwd).await.unwrap();
            reopened.reconcile("agent", "opencode", &history).unwrap();
            let rows = reopened.read("agent").unwrap().1;
            let mut turns: Vec<(String, String)> = Vec::new();
            for row in &rows {
                let text = row.entry.item["text"].as_str().unwrap();
                if row.entry.item["type"] == "user_message" {
                    turns.push((text.to_owned(), String::new()));
                } else {
                    assert_eq!(row.entry.item["type"], "assistant_message");
                    turns
                        .last_mut()
                        .expect("a user precedes every assistant")
                        .1
                        .push_str(text);
                }
            }
            let expected = (1..=number)
                .map(|index| (format!("question {index}"), "answer".to_owned()))
                .collect::<Vec<_>>();
            assert_eq!(turns, expected);
            session = client
                .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
                .await
                .unwrap();
        }
        session.close().await.unwrap();
        assert_eq!(fixture.state.lock().unwrap().submissions, 2);
    }
}

#[tokio::test]
async fn missing_admitted_input_never_publishes_an_orphan_assistant() {
    let fixture = Fixture::start(Version::V1).await;
    let client = OpenCodeClient::new(fixture.binary.clone());
    let spec = spec(&fixture);
    let mut session = client.create_session(&spec).await.unwrap();
    {
        let mut state = fixture.state.lock().unwrap();
        state.busy = true;
        state.omit_input_history = true;
        state.stream_events = stream_events(Version::V1, 1);
    }
    session.start_turn("hello", &spec.config).await.unwrap();
    let events = drain(session.as_mut()).await;
    assert!(matches!(events.as_slice(), [AgentTurnEvent::Failed]));
    session.close().await.unwrap();
    assert_eq!(fixture.state.lock().unwrap().submissions, 1);
}

#[tokio::test]
async fn legacy_opencode_projection_rebuilds_once_from_native_history() {
    for version in [Version::V1, Version::V2] {
        let fixture = Fixture::start(version).await;
        let client = OpenCodeClient::new(fixture.binary.clone());
        let spec = spec(&fixture);
        let mut session = client.create_session(&spec).await.unwrap();
        session.start_turn("hello", &spec.config).await.unwrap();
        assert!(matches!(
            drain(session.as_mut()).await.last(),
            Some(AgentTurnEvent::Completed(_))
        ));
        let handle = session.persistence().unwrap();
        session.close().await.unwrap();
        let history = client.history(&handle, &spec.cwd).await.unwrap();
        assert_eq!(history.len(), 2);
        let legacy = history
            .iter()
            .cloned()
            .map(|mut entry| {
                entry.key = entry
                    .key
                    .replace("native:opencode:projection-v3:", "native:opencode:");
                entry
            })
            .collect::<Vec<_>>();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("timeline.sqlite");
        let timeline = Timeline::open(&path).unwrap();
        let mut partial = legacy[1].clone();
        partial.item["text"] = json!("ans");
        timeline
            .progress("agent", "opencode", "early", &partial)
            .unwrap();
        let (old_epoch, _) = timeline.append("agent", "opencode", &legacy).unwrap();
        drop(timeline);
        let reopened = Timeline::open(&path).unwrap();
        assert_eq!(
            reopened.read("agent").unwrap().1[0].entry.item["type"],
            "assistant_message"
        );
        let epoch = reopened.reconcile("agent", "opencode", &history).unwrap();
        assert_ne!(epoch, old_epoch);
        let rows = reopened.read("agent").unwrap().1;
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].entry, history[0]);
        assert_eq!(rows[1].entry, history[1]);
        assert_eq!(rows[0].entry.item["type"], "user_message");
        assert_eq!(
            reopened.reconcile("agent", "opencode", &history).unwrap(),
            epoch
        );
        assert_eq!(fixture.state.lock().unwrap().submissions, 1);
    }
}
