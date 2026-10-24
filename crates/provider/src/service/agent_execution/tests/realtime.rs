use super::*;
use model::realtime::{Audio, Event};

async fn event(session: &mut model::realtime::Session) -> Event {
    tokio::time::timeout(Duration::from_secs(3), session.output.recv())
        .await
        .unwrap()
        .unwrap()
}

async fn stopped(fixture: &Fixture) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if fixture
                .requests()
                .iter()
                .any(|request| request["method"] == "thread/realtime/stop")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn realtime_canonical_transcripts_promotions_and_final_stop_items_survive_refresh() {
    let fixture = Fixture::new();
    fixture.mode("realtime-canonical");
    let (execution, _) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    let mut session = execution
        .start_realtime(id, CancellationToken::new(), None)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(event(&mut session).await, Event::Interrupted));
    assert!(matches!(event(&mut session).await, Event::Transcript(text) if text == "Hello!"));
    tokio::time::timeout(Duration::from_secs(3), async {
        while !fixture
            .requests()
            .iter()
            .any(|request| request["method"] == "thread/items/read")
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    drop(session);
    stopped(&fixture).await;
    let timeline =
        crate::storage::timeline::Timeline::open(&fixture.root.path().join("timeline.sqlite"))
            .unwrap();
    let before = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let (_, rows) = timeline.read(id).unwrap();
            let entries = crate::timeline_projection::project(&rows);
            if entries.iter().any(|entry| entry.item["status"] == "ended") {
                break entries;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        before
            .iter()
            .filter(|entry| entry.item["type"] == "voice")
            .count(),
        4
    );
    assert!(
        before
            .iter()
            .any(|entry| entry.item["text"] == "I can hear you.")
    );
    assert_eq!(
        before
            .iter()
            .filter(|entry| entry.item["type"] == "assistant_message")
            .count(),
        1
    );
    execution
        .execute("agent.refresh.request", json!({"agentId":id}))
        .await
        .unwrap();
    let (_, rows) = timeline.read(id).unwrap();
    let after = crate::timeline_projection::project(&rows);
    assert_eq!(after.len(), before.len());
    assert!(
        after
            .iter()
            .any(|entry| entry.item["text"] == "I can hear you.")
    );
    assert!(
        !fixture
            .requests()
            .iter()
            .any(|request| request["method"] == "turn/start")
    );
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn realtime_on_an_empty_agent_streams_audio_without_a_text_turn_and_stops_on_drop() {
    let fixture = Fixture::new();
    let (execution, _) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    assert_eq!(
        created["agent"]["capabilities"]["supportsRealtimeVoice"],
        true
    );
    let mut session = execution
        .start_realtime(id, CancellationToken::new(), None)
        .await
        .unwrap()
        .unwrap();
    assert!(
        execution
            .start_realtime(id, CancellationToken::new(), None)
            .await
            .is_err()
    );
    session
        .input
        .send(Audio {
            bytes: vec![0, 0, 0, 24, 0, 48],
            sample_rate: 16_000,
        })
        .await
        .unwrap();
    assert!(matches!(event(&mut session).await, Event::Interrupted));
    assert!(matches!(event(&mut session).await, Event::Transcript(text) if text == "hello"));
    let Event::Audio(audio) = event(&mut session).await else {
        panic!("native audio expected")
    };
    assert_eq!(audio.sample_rate, 24_000);
    assert_eq!(audio.bytes.len(), 8);
    assert!(session.output.try_recv().is_err());
    let requests = fixture.requests();
    assert!(
        !requests
            .iter()
            .any(|request| request["method"] == "turn/start")
    );
    let start = requests
        .iter()
        .find(|request| request["method"] == "thread/realtime/start")
        .unwrap();
    assert_eq!(start["params"]["outputModality"], "audio");
    assert_eq!(start["params"]["transport"]["type"], "websocket");
    assert_eq!(
        requests
            .iter()
            .filter(|request| request["method"] == "thread/start")
            .count(),
        1
    );
    drop(session);
    stopped(&fixture).await;
    let mut next = execution
        .start_realtime(id, CancellationToken::new(), None)
        .await
        .unwrap()
        .unwrap();
    next.input
        .send(Audio {
            bytes: vec![0, 8, 0, 8],
            sample_rate: 24_000,
        })
        .await
        .unwrap();
    assert!(matches!(event(&mut next).await, Event::Interrupted));
    assert!(matches!(event(&mut next).await, Event::Transcript(_)));
    assert!(matches!(event(&mut next).await, Event::Audio(_)));
    drop(next);
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn realtime_start_rejection_never_falls_back_to_a_text_turn() {
    let fixture = Fixture::new();
    fixture.mode("realtime-rejected");
    let (execution, _) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    assert!(
        execution
            .start_realtime(
                created["agentId"].as_str().unwrap(),
                CancellationToken::new(),
                None,
            )
            .await
            .is_err()
    );
    assert!(
        !fixture
            .requests()
            .iter()
            .any(|request| request["method"] == "turn/start")
    );
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn abandoned_realtime_start_cancels_after_native_admission() {
    let fixture = Fixture::new();
    fixture.mode("realtime-delayed");
    let (execution, _) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap().to_owned();
    let worker = execution.clone();
    let pending = tokio::spawn(async move {
        worker
            .start_realtime(&id, CancellationToken::new(), None)
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), async {
        while !fixture.cwd.join("realtime-started").exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    pending.abort();
    let _ = pending.await;
    std::fs::write(fixture.cwd.join("release-realtime"), "ready").unwrap();
    stopped(&fixture).await;
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn realtime_webrtc_negotiates_sdp_through_the_same_native_thread() {
    let fixture = Fixture::new();
    let (execution, _) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    let mut session = execution
        .start_realtime(
            id,
            CancellationToken::new(),
            Some("fixture-offer".to_owned()),
        )
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(event(&mut session).await, Event::Sdp(sdp) if sdp == "fixture-answer"));
    let requests = fixture.requests();
    let start = requests
        .iter()
        .find(|request| request["method"] == "thread/realtime/start")
        .unwrap();
    assert_eq!(
        start["params"]["transport"],
        json!({"type":"webrtc","sdp":"fixture-offer"})
    );
    assert!(
        !requests
            .iter()
            .any(|request| request["method"] == "turn/start")
    );
    drop(session);
    stopped(&fixture).await;
    execution.shutdown().await.unwrap();
}
