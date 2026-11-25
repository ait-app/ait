use super::*;

fn query(limit: usize) -> Value {
    json!({"scope":"active","includeObserved":true,"page":{"limit":limit}})
}

fn response(entries: Value, removals: Value, mode: &str) -> Value {
    let mut value = json!({"entries":[],"subscriptionId":"sub","pageInfo":{"hasMore":false,
        "nextCursor":null,"prevCursor":null},"sync":{"generation":"gen:observed",
        "headSeq":321,"mode":mode,"removals":[]}});
    value["entries"] = entries;
    value["sync"]["removals"] = removals;
    value
}

fn next_params(params: &Value, page: &Value) -> Value {
    let mut next = params.clone();
    next["page"]["cursor"] = page["pageInfo"]["nextCursor"].clone();
    next
}

#[test]
fn cold_sync_pages_fit_the_hub_and_return_the_same_checkpoint() {
    let rows: Vec<_> = (0..287)
        .map(|id| json!({"agent":{"id":id,"title":"研究\"".repeat(700)},"syncSeq":id+1}))
        .collect();
    let original = response(json!(rows), json!([]), "snapshot");
    assert!(serde_json::to_vec(&original).unwrap().len() > model::server::MAX_MESSAGE_BYTES);
    let mut pages = Pages::default();
    let params = query(200);
    let mut page = pages.first(&params, original).unwrap();
    assert_eq!(page["subscriptionId"], "sub");
    assert!(page["entries"].as_array().unwrap().len() < 200);
    let mut received = Vec::new();
    loop {
        let envelope = json!({"type":"response","request_id":"\\".repeat(128),"result":page});
        assert!(serde_json::to_vec(&envelope).unwrap().len() <= model::server::MAX_MESSAGE_BYTES);
        assert_eq!(page["sync"]["headSeq"], 321);
        assert_eq!(page["sync"]["mode"], "snapshot");
        received.extend(page["entries"].as_array().unwrap().iter().cloned());
        if page["pageInfo"]["hasMore"] == false {
            break;
        }
        let next = next_params(&params, &page);
        page = pages.resume(&next).unwrap().unwrap();
        assert!(page.get("subscriptionId").is_none());
        assert_eq!(pages.resume(&next).unwrap().unwrap(), page);
    }
    assert_eq!(received, rows);
}

#[test]
fn concurrent_reads_keep_live_changes_separate_from_the_original_pages() {
    let mut pages = Pages::default();
    let params = query(1);
    let first = pages
        .first(&params, response(json!([1, 2, 3]), json!([]), "snapshot"))
        .unwrap();
    let mut changes = response(json!([4]), json!([{"id":"gone","seq":322}]), "changes");
    changes["sync"]["headSeq"] = json!(322);
    let delta = pages.first(&params, changes).unwrap();
    let second = pages
        .resume(&next_params(&params, &first))
        .unwrap()
        .unwrap();
    let third = pages
        .resume(&next_params(&params, &second))
        .unwrap()
        .unwrap();
    assert_eq!(second["entries"], json!([2]));
    assert_eq!(third["entries"], json!([3]));
    assert_eq!(third["sync"]["headSeq"], 321);
    assert_eq!(delta["entries"], json!([4]));
    let tail = pages
        .resume(&next_params(&params, &delta))
        .unwrap()
        .unwrap();
    assert_eq!(tail["entries"], json!([]));
    assert_eq!(tail["sync"]["removals"], json!([{"id":"gone","seq":322}]));
    assert_eq!(tail["sync"]["mode"], "changes");
    assert_eq!(tail["sync"]["headSeq"], 322);
}

#[test]
fn cursors_are_scoped_to_their_connection_query_and_subscription() {
    let mut pages = Pages::default();
    let params = query(1);
    let first = pages
        .first(&params, response(json!([1, 2]), json!([]), "snapshot"))
        .unwrap();
    let next = next_params(&params, &first);
    assert_eq!(
        Pages::default().resume(&next),
        Some(Err(ErrorCode::InvalidMessage))
    );
    let mut changed = next.clone();
    changed["includeObserved"] = json!(false);
    assert_eq!(pages.resume(&changed), Some(Err(ErrorCode::InvalidMessage)));
    changed = next.clone();
    changed["filter"] = json!({"workspaceIds":["elsewhere"]});
    assert_eq!(pages.resume(&changed), Some(Err(ErrorCode::InvalidMessage)));
    pages.release("sub");
    assert_eq!(pages.resume(&next), Some(Err(ErrorCode::InvalidMessage)));
}

#[test]
fn oversized_row_returns_a_correlated_error_and_empty_reads_need_no_cursor() {
    let mut pages = Pages::default();
    let large = response(
        json!(["a".repeat(model::server::MAX_MESSAGE_BYTES)]),
        json!([]),
        "snapshot",
    );
    assert_eq!(
        pages.first(&query(200), large),
        Err(ErrorCode::ResourceExhausted)
    );
    let empty = response(json!([]), json!([]), "changes");
    assert_eq!(pages.first(&query(1), empty.clone()).unwrap(), empty);
    assert!(pages.reads.is_empty());
}
