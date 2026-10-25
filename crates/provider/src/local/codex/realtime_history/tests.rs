use super::*;

#[test]
fn history_interleaves_speech_and_structured_output_without_duplicate_handoffs() {
    let entries = json!([
        {"type":"realtime","item":{"type":"realtimeSessionStarted","id":"start","realtimeSessionId":"call"}},
        {"type":"realtime","item":{"type":"transcriptSegment","id":"u","realtimeSessionId":"call","role":"user","text":"hello"}},
        {"type":"turnStarted","turnId":"turn","startedAt":100},
        {"type":"item","turnId":"turn","item":{"id":"input","type":"userMessage","content":[{"type":"text","text":"<realtime_delegation><input>hello</input><transcript_delta>user: hello</transcript_delta></realtime_delegation>"}]}},
        {"type":"item","turnId":"turn","item":{"id":"answer","type":"agentMessage","text":"**Markdown**"}},
        {"type":"realtime","item":{"type":"bemItemPromoted","id":"p","realtimeSessionId":"call","turnId":"turn","itemId":"answer","presentation":{"type":"inlineMarkdown"}}},
        {"type":"realtime","item":{"type":"transcriptSegment","id":"a","realtimeSessionId":"call","role":"assistant","text":"spoken reply"}},
        {"type":"realtime","item":{"type":"realtimeSessionClosed","id":"end","realtimeSessionId":"call","outcome":"ended"}}
    ]);
    let output = project(
        entries.as_array().unwrap(),
        &json!({"createdAt":0}),
        &crate::local::images::ImageStore::default(),
    )
    .unwrap();
    assert_eq!(output.len(), 5);
    assert_eq!(output[1].item["role"], "user");
    assert_eq!(output[2].key, "native:turn:answer");
    assert_eq!(output[2].item["text"], "**Markdown**");
    assert_eq!(output[3].item["role"], "assistant");
    assert_eq!(output[4].item["status"], "ended");
}

#[tokio::test]
async fn reads_all_pages_and_only_falls_back_on_method_not_found() {
    let fixture = crate::test_support::Fixture::new();
    let client = fixture.client();
    let mut transport = client
        .launch_transport(fixture.cwd.to_str().unwrap(), false)
        .unwrap();
    transport.initialize().await.unwrap();
    let thread = json!({"id":"thread","createdAt":0});
    let images = crate::local::images::ImageStore::default();
    assert!(
        read(&mut transport, &thread, &images)
            .await
            .unwrap()
            .is_none()
    );
    let error_path = fixture.cwd.join("timeline-error.json");
    std::fs::write(&error_path, json!({"code":-32600,"message":"Invalid request: unknown variant `thread/timeline/list`, expected one of `initialize`, `thread/read`"}).to_string()).unwrap();
    assert!(
        read(&mut transport, &thread, &images)
            .await
            .unwrap()
            .is_none()
    );
    std::fs::write(
        &error_path,
        json!({"code":-32600,"message":"Invalid request: missing field threadId"}).to_string(),
    )
    .unwrap();
    assert_eq!(
        read(&mut transport, &thread, &images).await,
        Err(AgentSessionError::Rejected)
    );
    std::fs::remove_file(error_path).unwrap();
    let pages = json!({
        "first": {
            "data": [
                {"type":"realtime","position":3,"item":{"type":"transcriptSegment",
                    "id":"a","realtimeSessionId":"call","role":"assistant","text":"reply"}},
                {"type":"realtime","position":4,"item":{"type":"realtimeSessionClosed",
                    "id":"end","realtimeSessionId":"call","outcome":"ended"}}
            ],
            "nextCursor":"second","activeRealtimeSessionAtPageStart":"call"
        },
        "second": {
            "data": [
                {"type":"realtime","position":1,"item":{"type":"realtimeSessionStarted",
                    "id":"start","realtimeSessionId":"call"}},
                {"type":"realtime","position":2,"item":{"type":"transcriptSegment",
                    "id":"u","realtimeSessionId":"call","role":"user","text":"persisted"}}
            ],
            "nextCursor":null,"activeRealtimeSessionAtPageStart":null
        }
    });
    std::fs::write(fixture.cwd.join("timeline-pages.json"), pages.to_string()).unwrap();
    let entries = read(&mut transport, &thread, &images)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(entries.len(), 4);
    assert_eq!(entries[0].item["status"], "started");
    assert_eq!(entries[1].item["text"], "persisted");
    assert_eq!(entries[2].item["text"], "reply");
    assert_eq!(entries[3].item["status"], "ended");
    for (page, index, position) in [("first", 1, 3), ("first", 0, 5), ("second", 1, 3)] {
        let mut malformed = pages.clone();
        malformed[page]["data"][index]["position"] = json!(position);
        std::fs::write(
            fixture.cwd.join("timeline-pages.json"),
            malformed.to_string(),
        )
        .unwrap();
        assert!(read(&mut transport, &thread, &images).await.is_err());
    }
    let mut malformed = pages;
    malformed["second"]["nextCursor"] = json!("second");
    std::fs::write(
        fixture.cwd.join("timeline-pages.json"),
        malformed.to_string(),
    )
    .unwrap();
    assert!(read(&mut transport, &thread, &images).await.is_err());
    transport.close().await.unwrap();
}
