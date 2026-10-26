use super::*;

fn project(
    entries: &[Value],
    thread: &Value,
    images: &crate::local::images::ImageStore,
) -> Result<Vec<NativeItem>, AgentSessionError> {
    let created = native_sessions::timestamp(&thread["createdAt"])?;
    let mut projection = Projection::default();
    for (index, entry) in entries.iter().enumerate().rev() {
        let mut entry = entry.clone();
        entry["position"] = json!(index);
        projection.push(&entry, 1, &created, images)?;
    }
    projection.finish(&created, 1)
}

#[tokio::test]
async fn large_native_payload_is_projected_before_the_history_budget() {
    let fixture = crate::test_support::Fixture::new();
    let pages = json!({"first":{"data":[{"type":"item","position":1,"turnId":"turn",
        "item":{"id":"call","type":"mcpToolCall","server":"offline","tool":"capture",
            "status":"completed","arguments":{},"result":{"content":[
                {"type":"image","mimeType":"image/png","data":"iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aX1sAAAAASUVORK5CYII="}
            ],"_meta":{"opaque":"x".repeat(32 * 1024 * 1024)}}}}],"nextCursor":null}});
    serde_json::to_writer(
        std::fs::File::create(fixture.cwd.join("timeline-pages.json")).unwrap(),
        &pages,
    )
    .unwrap();
    drop(pages);
    let client = fixture.client();
    let mut transport = client
        .launch_transport(fixture.cwd.to_str().unwrap(), false)
        .unwrap();
    transport.initialize().await.unwrap();
    let images = crate::local::images::ImageStore::new(fixture.root.path().join("images"));
    let entries = read(
        &mut transport,
        &json!({"id":"thread","createdAt":0}),
        &images,
    )
    .await
    .expect("large native payloads must not prevent history reload")
    .unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].key, "native:turn:call");
    assert!(
        entries[1].item["text"]
            .as_str()
            .unwrap()
            .starts_with("![Image](")
    );
    assert!(serde_json::to_vec(&entries).unwrap().len() < 4096);
    assert_eq!(
        std::fs::read_dir(fixture.root.path().join("images"))
            .unwrap()
            .count(),
        1
    );
    transport.close().await.unwrap();
}

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
    assert!(matches!(
        read(&mut transport, &thread, &images).await,
        Err(AgentSessionError::History(HistoryError {
            page: 1,
            failure: HistoryFailure::Rpc("request rejected"),
            ..
        }))
    ));
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

#[tokio::test]
async fn image_heavy_history_exceeds_raw_budget_and_keeps_chronology_and_small_projection() {
    let fixture = crate::test_support::Fixture::new();
    assert!(crate::test_support::codex_history::write_large_history(&fixture) > MAX_BYTES as u64);
    let client = fixture
        .client()
        .with_image_directory(fixture.root.path().join("images"));
    std::fs::write(
        fixture.cwd.join("timeline-projection-check.json"),
        json!({"directory":fixture.root.path().join("images")}).to_string(),
    )
    .unwrap();
    let history = client
        .inspect_native("thread", fixture.cwd.to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(history.entries.len(), 130);
    for (index, entry) in history.entries.iter().enumerate() {
        assert_eq!(
            entry.key,
            format!("native:previous-turn:item-{}", index + 1)
        );
        assert_eq!(entry.timestamp, "2023-11-14T22:13:21+00:00");
        if index < 34 {
            let path = entry.item["imageSources"][0].as_str().unwrap();
            assert_eq!(std::fs::metadata(path).unwrap().len(), 54 + 512 * 512 * 3);
        }
    }
    assert!(serde_json::to_vec(&history.entries).unwrap().len() < 64 * 1024);
    assert!(
        fixture
            .requests()
            .iter()
            .filter(|request| request["method"] == "thread/read")
            .all(|request| request["params"]["includeTurns"] == false)
    );
    assert!(!fixture.requests().iter().any(|request| matches!(
        request["method"].as_str(),
        Some("thread/resume" | "turn/start" | "thread/rollback")
    )));
}

#[test]
fn projected_budget_accepts_the_exact_boundary_and_reports_one_extra_byte() {
    let images = crate::local::images::ImageStore::default();
    let created = "1970-01-01T00:00:00+00:00";
    let native = json!({"id":"a","type":"agentMessage","text":""});
    let overhead =
        display_bytes(&discovery::timeline_items(&native, "t", created, &images).unwrap()[0])
            .unwrap();
    for extra in [0, 1] {
        let entry = json!({"type":"item","position":1,"turnId":"t","item":{
            "id":"a","type":"agentMessage","text":"x".repeat(MAX_BYTES - overhead + extra)}});
        let mut projection = Projection::default();
        let result = projection.push(&entry, 3, created, &images);
        assert_eq!(projection.bytes, MAX_BYTES + extra);
        if extra == 0 {
            result.unwrap();
            assert_eq!(projection.finish(created, 3).unwrap().len(), 1);
        } else {
            let error = result.unwrap_err();
            assert_eq!(
                error,
                AgentSessionError::History(HistoryError {
                    method: METHOD,
                    page: 3,
                    bytes: MAX_BYTES + 1,
                    failure: HistoryFailure::Limit {
                        resource: "display bytes",
                        limit: MAX_BYTES,
                        actual: MAX_BYTES + 1
                    }
                })
            );
            let message = error.to_string();
            assert!(message.contains("thread/timeline/list, page 3"));
            assert!(message.contains("33554432"));
            assert!(message.contains("33554433"));
            assert!(message.contains("native client"));
        }
    }
}

#[test]
fn malformed_history_and_entry_limits_are_distinct_from_display_limits() {
    let images = crate::local::images::ImageStore::default();
    let created = "1970-01-01T00:00:00+00:00";
    let duplicate = json!([
        {"type":"item","turnId":"t","item":{"id":"same","type":"agentMessage","text":"first"}},
        {"type":"item","turnId":"t","item":{"id":"same","type":"agentMessage","text":"second"}}
    ]);
    assert!(matches!(
        project(
            duplicate.as_array().unwrap(),
            &json!({"createdAt":0}),
            &images
        ),
        Err(AgentSessionError::History(HistoryError {
            failure: HistoryFailure::Protocol(_),
            ..
        }))
    ));
    let mut projection = Projection {
        count: MAX_ENTRIES,
        ..Default::default()
    };
    assert!(matches!(
        projection.push(&json!({}), 2, created, &images),
        Err(AgentSessionError::History(HistoryError {
            page: 2,
            failure: HistoryFailure::Limit {
                resource: "canonical entries",
                limit: MAX_ENTRIES,
                actual: 32769
            },
            ..
        }))
    ));
}

#[test]
fn promotions_and_voice_handoffs_resolve_across_reverse_pages() {
    let entries = json!([
        {"type":"turnStarted","turnId":"turn","startedAt":100},
        {"type":"realtime","item":{"type":"bemItemPromoted","id":"p","turnId":"turn","itemId":"answer"}},
        {"type":"item","turnId":"turn","item":{"id":"input","type":"userMessage","content":[{"type":"text","text":"<realtime_delegation><input>hello</input><transcript_delta>user: hello</transcript_delta></realtime_delegation>"}]}},
        {"type":"realtime","item":{"type":"transcriptSegment","id":"u","realtimeSessionId":"call","role":"user","text":"hello"}},
        {"type":"item","turnId":"turn","item":{"id":"answer","type":"agentMessage","text":"**Markdown**"}}
    ]);
    let output = project(
        entries.as_array().unwrap(),
        &json!({"createdAt":0}),
        &crate::local::images::ImageStore::default(),
    )
    .unwrap();
    assert_eq!(output.len(), 2);
    assert_eq!(output[0].key, "native:turn:answer");
    assert_eq!(output[0].item["text"], "**Markdown**");
    assert_eq!(output[1].item["text"], "hello");
    assert!(
        output
            .iter()
            .all(|item| item.timestamp == "1970-01-01T00:01:40+00:00")
    );
}

#[tokio::test]
async fn malformed_pages_late_rpc_failures_and_page_limits_do_not_fall_back() {
    let fixture = crate::test_support::Fixture::new();
    let client = fixture.client();
    let mut transport = client
        .launch_transport(fixture.cwd.to_str().unwrap(), false)
        .unwrap();
    transport.initialize().await.unwrap();
    let thread = json!({"id":"thread","createdAt":0});
    let images = crate::local::images::ImageStore::default();
    let path = fixture.cwd.join("timeline-pages.json");
    for first in [
        json!(42),
        json!({"data":[]}),
        json!({"data":[],"nextCursor":""}),
        json!({"data":[{"type":"turnCompleted","position":-1}],"nextCursor":null}),
    ] {
        std::fs::write(&path, json!({"first":first}).to_string()).unwrap();
        assert!(matches!(
            read(&mut transport, &thread, &images).await,
            Err(AgentSessionError::History(HistoryError {
                page: 1,
                failure: HistoryFailure::Protocol(_),
                ..
            }))
        ));
    }
    std::fs::write(
        &path,
        json!({"first":{"data":[],"nextCursor":"second"},
        "second":{"rpcError":{"code":-32601,"message":"Method not found"}}})
        .to_string(),
    )
    .unwrap();
    assert!(matches!(
        read(&mut transport, &thread, &images).await,
        Err(AgentSessionError::History(HistoryError {
            page: 2,
            failure: HistoryFailure::Rpc(_),
            ..
        }))
    ));
    let mut pages = serde_json::Map::new();
    for page in 0..MAX_PAGES {
        let key = if page == 0 {
            "first".to_owned()
        } else {
            page.to_string()
        };
        pages.insert(key, json!({"data":[],"nextCursor":(page + 1).to_string()}));
    }
    std::fs::write(&path, Value::Object(pages).to_string()).unwrap();
    assert_eq!(
        read(&mut transport, &thread, &images).await,
        Err(AgentSessionError::History(HistoryError {
            method: METHOD,
            page: MAX_PAGES,
            bytes: 0,
            failure: HistoryFailure::Limit {
                resource: "pages",
                limit: MAX_PAGES,
                actual: MAX_PAGES + 1
            }
        }))
    );
    transport.close().await.unwrap();
}
