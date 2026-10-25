//! Explicit opt-in smoke test; never runs in offline CI or captures a microphone.

use super::*;
use crate::local::codex::transport::Transport;
use serde_json::json;

use crate::local::codex::CodexClient;
use crate::ports::agent_session::{
    AgentClient, AgentResumePurpose, AgentSession, AgentSessionSpec, AgentTurnEvent,
};

#[tokio::test]
#[ignore = "requires logged-in Codex and AIT_CODEX_REALTIME_PCM pointing to synthetic mono PCM16 at 24 kHz"]
async fn logged_in_codex_speaks_after_hangup_and_after_provider_restart() {
    let pcm = std::fs::read(std::env::var_os("AIT_CODEX_REALTIME_PCM").unwrap()).unwrap();
    assert!(!pcm.is_empty() && pcm.len() < 24_000 * 2 * 20 && pcm.len().is_multiple_of(2));
    let cwd = tempfile::tempdir().unwrap();
    let spec = AgentSessionSpec {
        provider: "codex".to_owned(),
        cwd: cwd.path().to_str().unwrap().to_owned(),
        config: domain::agent_runtime::StoredAgentConfig::default(),
    };
    let client = CodexClient::new("codex".into());
    let mut native = client.create_session(&spec).await.unwrap();
    let handle = native.persistence().unwrap();
    for round in 0..3 {
        if round == 2 {
            native.close().await.unwrap();
            native = client
                .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
                .await
                .unwrap();
        }
        tokio::time::timeout(Duration::from_secs(60), speak(native.as_mut(), &pcm))
            .await
            .unwrap_or_else(|_| panic!("native voice round {round} timed out"));
        assert_eq!(native.persistence().unwrap().session_id, handle.session_id);
    }
    native.close().await.unwrap();
}

async fn speak(native: &mut dyn AgentSession, pcm: &[u8]) {
    let (mut voice, endpoint) = model::realtime::channel(&CancellationToken::new());
    native.start_realtime(endpoint).await.unwrap();
    let mut ready = false;
    let mut offset = 0;
    let mut next_input = tokio::time::Instant::now();
    let mut user_transcribed = false;
    let mut user_turn_completed = false;
    let mut completed = None;
    let mut audible_samples = 0;
    loop {
        native.service_realtime().await.unwrap();
        while let Some(event) = native.poll_turn().unwrap() {
            match event {
                AgentTurnEvent::Timeline(entry)
                    if entry.item["type"] == "voice"
                        && entry.item["role"] == "user"
                        && entry.item["status"] == "completed" =>
                {
                    user_turn_completed = true;
                }
                AgentTurnEvent::Timeline(entry)
                    if entry.item["type"] == "voice"
                        && entry.item["role"] == "assistant"
                        && entry.item["status"] == "completed"
                        && user_turn_completed =>
                {
                    completed = Some(tokio::time::Instant::now());
                }
                AgentTurnEvent::Failed => panic!("native conversation failed"),
                _ => {}
            }
        }
        if !ready && voice.ready.try_recv().is_ok() {
            ready = true;
            next_input = tokio::time::Instant::now();
        }
        if ready && offset < pcm.len() && tokio::time::Instant::now() >= next_input {
            let end = (offset + 960).min(pcm.len());
            voice
                .input
                .try_send(model::realtime::Audio {
                    bytes: pcm[offset..end].to_vec(),
                    sample_rate: 24_000,
                })
                .unwrap();
            offset = end;
            next_input = tokio::time::Instant::now() + Duration::from_millis(20);
        }
        while let Ok(event) = voice.output.try_recv() {
            match event {
                Event::Audio(audio) => {
                    assert_eq!(audio.sample_rate, 24_000);
                    audible_samples += audio
                        .bytes
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .filter(|sample| i16::from_le_bytes(**sample).unsigned_abs() > 100)
                        .count();
                }
                Event::Transcript(text) => user_transcribed |= !text.is_empty(),
                Event::Interrupted => {}
                Event::Failed | Event::Closed => {
                    panic!("native voice ended before its reply completed")
                }
            }
        }
        if user_transcribed
            && audible_samples > 12_000
            && completed.is_some_and(|time| time.elapsed() > Duration::from_millis(500))
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    drop(voice);
    native.service_realtime().await.unwrap();
    assert!(!native.realtime_active());
}

#[tokio::test]
#[ignore = "requires installed Codex with login, upstream network and realtime entitlement"]
async fn logged_in_codex_connects_the_provider_owned_media_peer() {
    let cwd = tempfile::tempdir().unwrap();
    let mut native = Transport::spawn(
        std::path::Path::new("codex"),
        cwd.path().to_str().unwrap(),
        Duration::from_secs(25),
    )
    .unwrap();
    native.initialize().await.unwrap();
    let result = native
        .request(
            "thread/start",
            json!({
                "cwd": cwd.path(), "ephemeral":true, "approvalPolicy":"never", "sandbox":"read-only"
            }),
        )
        .await
        .unwrap();
    let thread = result["thread"]["id"].as_str().unwrap();
    let (mut session, endpoint) = model::realtime::channel(&CancellationToken::new());
    let mut media = open(endpoint).await.unwrap();
    native
        .request(
            "thread/realtime/start",
            json!({
                "threadId":thread,"version":"v3","outputModality":"audio",
                "transport":{"type":"webrtc","sdp":media.offer}
            }),
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(25), async {
        loop {
            if let Some(event) = native.poll().unwrap() {
                match event["method"].as_str() {
                    Some("thread/realtime/sdp") => {
                        media
                            .answer(event["params"]["sdp"].as_str().unwrap().to_owned())
                            .unwrap();
                        break;
                    }
                    Some("thread/realtime/error" | "thread/realtime/closed") => {
                        panic!("native rejected realtime startup")
                    }
                    _ => {}
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        (&mut session.ready).await.unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(!media.cancel.is_cancelled());
    })
    .await
    .unwrap();
    drop(session);
    native
        .request("thread/realtime/stop", json!({"threadId":thread}))
        .await
        .unwrap();
    native.close().await.unwrap();
}
