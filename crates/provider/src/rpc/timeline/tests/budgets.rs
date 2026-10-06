use super::*;

fn tool(seq: u64, call: &str, bytes: usize, status: &str) -> Row {
    Row {
        seq,
        provider: "codex".to_owned(),
        entry: NativeItem {
            key: format!("native:turn:{seq}"),
            turn_id: Some("turn".to_owned()),
            timestamp: "2026-10-06T00:00:00Z".to_owned(),
            item: json!({"type":"tool_call","callId":call,"name":"mcp.logs",
                "status":status,"error":null,"detail":{"type":"unknown","input":{},
                "output":"x".repeat(bytes)}}),
        },
    }
}

fn page(rows: &[Row], direction: &str, cursor: Option<u64>, limit: usize) -> Value {
    let mut input = json!({"agentId":"a","direction":direction,"limit":limit});
    if let Some(seq) = cursor {
        input["cursor"] = json!({"epoch":"e","seq":seq});
    }
    let value = fetch(&request(input), "e", rows, &Value::Null).unwrap();
    assert!(serde_json::to_vec(&value).unwrap().len() <= MAX_RESPONSE_BYTES);
    value
}

#[test]
fn large_results_page_in_both_directions_without_losing_payloads_or_cursors() {
    let size = crate::storage::timeline::MAX_ENTRY_BYTES - 1024;
    let rows = vec![
        tool(1, "one", size, "completed"),
        tool(2, "two", size, "completed"),
        tool(3, "three", size, "completed"),
    ];
    for limit in [0, 40] {
        let tail = page(&rows, "tail", None, limit);
        assert_eq!(tail["startCursor"]["seq"], 3);
        assert_eq!(tail["endCursor"]["seq"], 3);
        assert_eq!(tail["hasOlder"], true);
        assert_eq!(tail["hasNewer"], false);
        for seq in (1..=3).rev() {
            let previous = page(&rows, "before", Some(seq + 1), limit);
            assert_eq!(previous["startCursor"]["seq"], seq);
            assert_eq!(previous["endCursor"]["seq"], seq);
            assert_eq!(previous["hasOlder"], seq > 1);
            assert_eq!(previous["hasNewer"], seq < 3);
            assert_eq!(
                previous["entries"][0]["item"]["detail"]["output"]
                    .as_str()
                    .unwrap()
                    .len(),
                size
            );
        }
        for seq in 1..=3 {
            let next = page(&rows, "after", Some(seq - 1), limit);
            assert_eq!(next["startCursor"]["seq"], seq);
            assert_eq!(next["endCursor"]["seq"], seq);
            assert_eq!(next["hasNewer"], seq < 3);
            assert_eq!(
                next["entries"][0]["item"]["detail"]["output"]
                    .as_str()
                    .unwrap()
                    .len(),
                size
            );
        }
    }
}

#[test]
fn interleaved_large_tool_lifecycles_split_at_source_boundaries() {
    let rows = vec![
        tool(1, "one", 0, "running"),
        tool(2, "two", 0, "running"),
        tool(3, "one", 700 * 1024, "completed"),
        tool(4, "two", 700 * 1024, "completed"),
    ];
    let tail = page(&rows, "tail", None, 40);
    assert_eq!(tail["startCursor"]["seq"], 4);
    assert_eq!(tail["entries"][0]["item"]["callId"], "two");
    let previous = page(&rows, "before", Some(4), 40);
    assert_eq!(previous["startCursor"]["seq"], 1);
    assert_eq!(previous["endCursor"]["seq"], 3);
    assert_eq!(previous["entries"][0]["item"]["callId"], "one");
    assert_eq!(previous["entries"][0]["item"]["status"], "completed");
    assert_eq!(previous["entries"][1]["item"]["status"], "running");
    let first = page(&rows, "after", Some(0), 0);
    assert_eq!(first["startCursor"]["seq"], 1);
    assert_eq!(first["endCursor"]["seq"], 3);
    assert_eq!(first["hasNewer"], true);
    assert_eq!(first["entries"][0]["item"]["status"], "completed");
    assert_eq!(first["entries"][1]["item"]["status"], "running");
    let next = page(&rows, "after", Some(3), 0);
    assert_eq!(next["endCursor"]["seq"], 4);
    assert_eq!(next["entries"][0]["item"]["status"], "completed");
    assert_eq!(next["hasNewer"], false);
}

#[test]
fn response_budget_accounts_for_agent_metadata_and_stale_cursor_resets() {
    let rows = vec![
        tool(1, "one", 700 * 1024, "completed"),
        tool(2, "two", 700 * 1024, "completed"),
    ];
    let value = fetch(
        &request(json!({"agentId":"a","direction":"after","cursor":{"epoch":"old","seq":1},"mergeWindow":true})),
        "e", &rows, &json!({"metadata":"x".repeat(100 * 1024)}),
    ).unwrap();
    assert!(serde_json::to_vec(&value).unwrap().len() <= MAX_RESPONSE_BYTES);
    assert_eq!(value["reset"], true);
    assert_eq!(value["mergeWindow"], true);
    assert_eq!(value["startCursor"]["seq"], 2);
    assert_eq!(value["hasOlder"], true);
    assert_eq!(value["hasNewer"], false);
    assert_eq!(
        fetch(
            &request(json!({"agentId":"a"})),
            "e",
            &rows,
            &json!({"metadata":"x".repeat(250 * 1024)})
        ),
        Err(ErrorCode::ResourceExhausted)
    );
}

#[test]
fn merged_text_is_split_into_complete_source_rows_instead_of_truncated() {
    let rows = (1..=3)
        .map(|seq| {
            let mut row = tool(seq, "unused", 0, "completed");
            row.entry.item = json!({"type":"reasoning","text":"界".repeat(128 * 1024)});
            row
        })
        .collect::<Vec<_>>();
    let first = page(&rows, "after", Some(0), 0);
    assert_eq!(first["endCursor"]["seq"], 2);
    assert_eq!(
        first["entries"][0]["item"]["text"].as_str().unwrap().len(),
        768 * 1024
    );
    let next = page(&rows, "after", Some(2), 0);
    assert_eq!(next["startCursor"]["seq"], 3);
    assert_eq!(next["endCursor"]["seq"], 3);
    assert_eq!(next["hasNewer"], false);
}
