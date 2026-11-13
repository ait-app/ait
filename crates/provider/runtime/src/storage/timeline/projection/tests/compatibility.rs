use super::*;

#[test]
fn repaired_process_cache_respects_the_page_byte_budget() {
    let timeline = Timeline::memory().unwrap();
    let entries: Vec<_> = (1..=2)
        .map(|seq| {
            item(
                seq,
                json!({"type":"tool_call",
        "callId":seq.to_string(),"name":"write","status":"completed","error":null,
        "detail":{"type":"write","filePath":"proc://collector/kill","content":null}}),
            )
        })
        .collect();
    timeline.append("agent", "omp", &entries).unwrap();
    let (epoch, rows) = timeline.read("agent").unwrap();
    let budget = install_legacy_cache(&timeline, &rows);
    for (direction, cursor) in [("tail", 2), ("before", 3), ("after", 0)] {
        let request: FetchRequest = serde_json::from_value(json!({"agentId":"agent",
            "direction":direction,"limit":2,"cursor":{"epoch":epoch,"seq":cursor}}))
        .unwrap();
        let page = timeline.page(&request, "agent", |_, _| Ok(budget)).unwrap();
        assert_eq!(page.entries.len(), 1);
        assert!(serde_json::to_vec(&page.entries).unwrap().len() <= budget);
        assert_eq!(page.entries[0].item["detail"]["type"], "unknown");
        assert_eq!(
            page.entries[0].seq_start,
            if direction == "after" { 1 } else { 2 }
        );
        assert_eq!(page.has_older, direction != "after");
        assert_eq!(page.has_newer, direction == "after");
    }
    assert_eq!(timeline.read("agent").unwrap(), (epoch, rows));
}

#[test]
fn legacy_write_pages_survive_reopen_rebuild_and_incremental_lifecycle_updates() {
    for rebuild in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("timeline.sqlite");
        let timeline = Timeline::open(&path).unwrap();
        let originals = [
            item(
                1,
                json!({"type":"tool_call","callId":"file","name":"write",
                "status":"completed","error":null,
                "detail":{"type":"write","filePath":"file.rs","content":null}}),
            ),
            item(
                2,
                json!({"type":"tool_call","callId":"process","name":"write",
                "status":"running","error":null,
                "detail":{"type":"write","filePath":"proc://collector/kill","content":null}}),
            ),
            item(3, json!({"type":"assistant_message","text":"stopping"})),
        ];
        timeline.append("agent", "omp", &originals).unwrap();
        let (epoch, canonical) = timeline.read("agent").unwrap();
        if rebuild {
            clear(&timeline.database.lock().unwrap(), "agent").unwrap();
        } else {
            install_legacy_cache(&timeline, &canonical);
        }
        drop(timeline);
        let timeline = Timeline::open(&path).unwrap();
        for (direction, cursor) in [("tail", 3), ("before", 4), ("after", 0)] {
            let request: FetchRequest = serde_json::from_value(json!({"agentId":"agent",
                "direction":direction,"limit":20,"cursor":{"epoch":epoch,"seq":cursor}}))
            .unwrap();
            let response = fetch_stored(&request, &timeline, "agent", &Value::Null, None).unwrap();
            assert_eq!(response["epoch"], epoch);
            assert_eq!(response["reset"], false);
            assert_eq!(response["entries"].as_array().unwrap().len(), 3);
            assert!(
                response["entries"][0]["item"]["detail"]
                    .get("content")
                    .is_none()
            );
            assert_eq!(response["entries"][1]["item"]["detail"]["type"], "unknown");
            assert_eq!(
                response["entries"][1]["item"]["detail"]["input"]["filePath"],
                "proc://collector/kill"
            );
            assert_eq!(
                response["entries"][1]["sourceSeqRanges"],
                json!([{"startSeq":2,"endSeq":2}])
            );
        }
        assert_eq!(
            timeline.read("agent").unwrap(),
            (epoch.clone(), canonical.clone())
        );

        let completed = item(
            4,
            json!({"type":"tool_call","callId":"process","name":"write",
            "status":"canceled","error":null,"detail":{"type":"unknown",
            "input":{"file_path":"proc://collector/kill","content":null},"output":{"stopped":true}}}),
        );
        timeline.append("agent", "omp", &[completed]).unwrap();
        let request: FetchRequest = serde_json::from_value(json!({"agentId":"agent",
            "direction":"after","cursor":{"epoch":epoch,"seq":3}}))
        .unwrap();
        let response = fetch_stored(&request, &timeline, "agent", &Value::Null, None).unwrap();
        assert_eq!(response["epoch"], epoch);
        assert_eq!(response["reset"], false);
        assert_eq!(response["endCursor"]["seq"], 4);
        assert_eq!(response["entries"].as_array().unwrap().len(), 1);
        assert_eq!(response["entries"][0]["item"]["status"], "canceled");
        assert_eq!(
            response["entries"][0]["item"]["detail"]["output"],
            json!({"stopped":true})
        );
        assert_eq!(
            response["entries"][0]["sourceSeqRanges"],
            json!([
            {"startSeq":2,"endSeq":2},{"startSeq":4,"endSeq":4}])
        );
        let (next_epoch, next_rows) = timeline.read("agent").unwrap();
        assert_eq!(next_epoch, epoch);
        assert_eq!(&next_rows[..canonical.len()], canonical);
    }
}

// Restore the old display bytes and byte counts without changing any canonical source rows.
fn install_legacy_cache(timeline: &Timeline, rows: &[Row]) -> usize {
    let mut bytes = 2 + rows.len().saturating_sub(1);
    let database = timeline.database.lock().unwrap();
    for row in rows {
        let mut cached = serde_json::to_value(Entry::from(row)).unwrap();
        cached["item"] = row.entry.item.clone();
        let encoded = serde_json::to_string(&cached).unwrap();
        bytes += encoded.len();
        database
            .execute(
                "UPDATE display_entries SET entry=?,bytes=? WHERE agent=? AND seq_start=?",
                params![encoded, encoded.len(), "agent", row.seq],
            )
            .unwrap();
        database
            .execute(
                "UPDATE display_sources SET bytes=? WHERE agent=? AND seq=?",
                params![encoded.len(), "agent", row.seq],
            )
            .unwrap();
    }
    bytes
}
