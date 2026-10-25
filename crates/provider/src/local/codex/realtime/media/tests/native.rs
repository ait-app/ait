//! Explicit opt-in smoke test; never runs in offline CI or captures a microphone.

use super::*;
use crate::local::codex::transport::Transport;
use serde_json::json;

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
