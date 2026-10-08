use super::*;
use crate::storage::timeline::Timeline;

fn tool(version: Version, number: usize, denied: bool) -> Value {
    let status = if denied { "error" } else { "completed" };
    match version {
        Version::V1 => {
            json!({"id":format!("part-tool-{number}"),"messageID":"tools","sessionID":"ses_one","type":"tool","callID":format!("call-{number}"),"tool":"bash","state":{"status":status,"input":{"command":"pwd"},"output":"/project"}})
        }
        Version::V2 => {
            json!({"id":format!("call-{number}"),"type":"tool","name":"shell","state":{"status":status,"input":{"command":"pwd"},"content":[{"type":"text","text":"/project"}]}})
        }
    }
}

fn tool_message(version: Version, denied: bool) -> Value {
    match version {
        Version::V1 => {
            json!({"info":{"id":"tools","sessionID":"ses_one","role":"assistant","time":{"created":11,"completed":12}},"parts":[{"id":"preface","messageID":"tools","sessionID":"ses_one","type":"text","text":"checking"},tool(version,1,denied),tool(version,2,denied)]})
        }
        Version::V2 => {
            json!({"id":"tools","type":"assistant","time":{"created":11,"completed":12},"content":[{"type":"text","text":"checking"},tool(version,1,denied),tool(version,2,denied)]})
        }
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
        AgentTurnEvent::Failed => panic!("native tool turn failed"),
        _ => {}
    }
}

async fn next(session: &mut dyn AgentSession) -> AgentTurnEvent {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Some(event) = session.poll_turn().unwrap() {
                return event;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap()
}

async fn run(version: Version, denied: bool, same_message: bool, stream_preface: bool) {
    let fixture = Fixture::start(version).await;
    let client = OpenCodeClient::new(fixture.binary.clone());
    let spec = spec(&fixture);
    let mut session = client.create_session(&spec).await.unwrap();
    configure_stream(&fixture, version, stream_preface);
    session
        .start_input(
            &AgentPrompt {
                text: "please check".into(),
                client_message_id: Some("client".into()),
                ..Default::default()
            },
            &spec.config,
        )
        .await
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("timeline.sqlite");
    let timeline = Timeline::open(&path).unwrap();
    let permission = next(session.as_mut()).await;
    assert!(
        matches!(permission, AgentTurnEvent::PermissionRequested(_)),
        "{permission:?}"
    );
    prepare_history(&fixture, version, denied, same_message);
    session
        .respond_permission(
            "perm1",
            &json!({"behavior":if denied {"deny"} else {"allow"}}),
        )
        .await
        .unwrap();
    if !denied {
        loop {
            let event = next(session.as_mut()).await;
            let progress = matches!(&event, AgentTurnEvent::Progress { entry, .. } if entry.item["text"] == "ans");
            persist(&timeline, event);
            if progress {
                break;
            }
        }
        verify_stream_order(&timeline, denied, stream_preface);
        let mut state = fixture.state.lock().unwrap();
        state.busy = false;
        let answer = state.history.last_mut().unwrap();
        if version == Version::V1 {
            answer["info"]["time"]["completed"] = json!(13);
        } else {
            answer["time"]["completed"] = json!(13);
        }
    }
    let finished = drain(session.as_mut()).await;
    if denied {
        assert!(matches!(finished.last(), Some(AgentTurnEvent::Cancelled)));
        assert_eq!(fixture.state.lock().unwrap().interrupts, 1);
    } else {
        assert!(matches!(
            finished.last(),
            Some(AgentTurnEvent::Completed(_))
        ));
    }
    for event in finished {
        persist(&timeline, event);
    }
    if denied {
        verify_stream_order(&timeline, denied, stream_preface);
    }
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let history = client.history(&handle, &spec.cwd).await.unwrap();
    verify_restart(timeline, &path, &history);
    assert_eq!(fixture.state.lock().unwrap().submissions, 1);
    legacy_projection_recovers(&history);
}

#[tokio::test]
async fn completed_tools_precede_streamed_conclusions_after_allow_deny_and_restart() {
    for version in [Version::V1, Version::V2] {
        for denied in [false, true] {
            run(version, denied, false, false).await;
        }
    }
}

#[tokio::test]
async fn same_message_tools_precede_conclusion_while_message_is_still_streaming() {
    run(Version::V1, false, true, false).await;
}

#[tokio::test]
async fn streamed_preface_finishes_before_tools_and_late_deltas_do_not_replay_it() {
    run(Version::V1, false, true, true).await;
}

fn legacy_projection_recovers(history: &[NativeItem]) {
    for prefix in ["native:opencode:", "native:opencode:projection-v2:"] {
        let timeline = Timeline::memory().unwrap();
        let legacy = history
            .iter()
            .cloned()
            .map(|mut entry| {
                entry.key = entry.key.replace("native:opencode:projection-v3:", prefix);
                entry
            })
            .collect::<Vec<_>>();
        assert_ne!(legacy[0].key, history[0].key);
        timeline.append("agent", "opencode", &legacy[..1]).unwrap();
        let mut answer = legacy.last().unwrap().clone();
        answer.item["text"] = json!("ans");
        timeline
            .progress("agent", "opencode", "early", &answer)
            .unwrap();
        let (before, _) = timeline.append("agent", "opencode", &legacy[1..]).unwrap();
        let after = timeline.reconcile("agent", "opencode", history).unwrap();
        assert_ne!(after, before);
        assert_eq!(
            timeline
                .read("agent")
                .unwrap()
                .1
                .into_iter()
                .map(|row| row.entry)
                .collect::<Vec<_>>(),
            history
        );
        assert_eq!(
            timeline.reconcile("agent", "opencode", history).unwrap(),
            after
        );
    }
}

fn configure_stream(fixture: &Fixture, version: Version, stream_preface: bool) {
    {
        let mut state = fixture.state.lock().unwrap();
        state.stream_after_permission = true;
        state.pending_permissions.push(match version {
            Version::V1 => {
                json!({"id":"perm1","sessionID":"ses_one","permission":"bash","patterns":["pwd"]})
            }
            Version::V2 => {
                json!({"id":"perm1","sessionID":"ses_one","action":"shell","resources":["pwd"]})
            }
        });
        state.stream_events = match version {
            Version::V1 => vec![
                json!({"type":"message.updated","properties":{"info":{"id":"msg_0123456789abABCDEFGHIJKLM1","sessionID":"ses_one","role":"assistant"}}}),
                json!({"type":"message.part.updated","properties":{"part":{"id":"prt_0123456789abABCDEFGHIJKLM1","messageID":"msg_0123456789abABCDEFGHIJKLM1","sessionID":"ses_one","type":"text","text":"ans"}}}),
            ],
            Version::V2 => vec![
                json!({"type":"session.text.delta","data":{"sessionID":"ses_one","assistantMessageID":"answer1","delta":"ans"}}),
            ],
        };
    }
    if stream_preface {
        let mut state = fixture.state.lock().unwrap();
        let preface = |text: &str| json!({"type":"message.part.updated","properties":{"part":{"id":"preface","messageID":"msg_0123456789abABCDEFGHIJKLM1","sessionID":"ses_one","type":"text","text":text}}});
        state.stream_events.insert(1, preface("check"));
        state.stream_events.push(preface("checking"));
    }
}

fn prepare_history(fixture: &Fixture, version: Version, denied: bool, same_message: bool) {
    {
        let mut state = fixture.state.lock().unwrap();
        let mut answer = state.history.pop().unwrap();
        let mut tools = tool_message(version, denied);
        if same_message {
            let id = answer["info"]["id"].clone();
            let mut parts = tools["parts"].as_array_mut().unwrap().clone();
            for part in &mut parts {
                part["messageID"] = id.clone();
            }
            parts.extend(answer["parts"].as_array().unwrap().iter().cloned());
            answer["parts"] = json!(parts);
        } else {
            state.history.push(tools);
        }
        if version == Version::V1 {
            answer["info"]["time"]
                .as_object_mut()
                .unwrap()
                .remove("completed");
        } else {
            answer["time"].as_object_mut().unwrap().remove("completed");
        }
        state.history.push(answer);
    }
}

fn verify_restart(timeline: Timeline, path: &std::path::Path, history: &[NativeItem]) {
    let before = timeline.read("agent").unwrap();
    drop(timeline);
    let reopened = Timeline::open(path).unwrap();
    let after = reopened.read("agent").unwrap();
    assert_eq!(
        after
            .1
            .iter()
            .map(crate::storage::timeline::Row::value)
            .collect::<Vec<_>>(),
        before
            .1
            .iter()
            .map(crate::storage::timeline::Row::value)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        reopened.reconcile("agent", "opencode", history).unwrap(),
        before.0
    );
    let rows = reopened.read("agent").unwrap().1;
    assert_eq!(
        rows.iter()
            .filter(|row| row.entry.item["type"] == "tool_call")
            .count(),
        2
    );
    let tool_end = rows
        .iter()
        .rposition(|row| row.entry.item["type"] == "tool_call")
        .unwrap();
    assert_eq!(rows[tool_end + 1].entry.item["type"], "assistant_message");
    assert_eq!(
        rows[tool_end + 1..]
            .iter()
            .map(|row| row.entry.item["text"].as_str().unwrap())
            .collect::<String>(),
        "answer"
    );
}

fn verify_stream_order(timeline: &Timeline, denied: bool, stream_preface: bool) {
    let rows = timeline.read("agent").unwrap().1;
    assert_eq!(
        rows.iter()
            .filter(|row| !(stream_preface && row.entry.item["text"] == "ing"))
            .map(|row| row.entry.item["type"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "user_message",
            "assistant_message",
            "tool_call",
            "tool_call",
            "assistant_message"
        ]
    );
    assert_eq!(rows[0].entry.item["clientMessageId"], "client");
    assert_eq!(
        rows.iter()
            .find(|row| row.entry.item["type"] == "tool_call")
            .unwrap()
            .entry
            .item["status"],
        if denied { "failed" } else { "completed" }
    );
}
