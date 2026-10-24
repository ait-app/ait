use super::*;

#[test]
fn realtime_delegation_projects_conversation_without_xml_or_duplicate_input() {
    let text = "<realtime_delegation>\n  <input>能进入这个 thread 吗？</input>\n  <transcript_delta>user: 试试看\nassistant: 我能听到你。\nuser: 能进入这个 thread 吗？</transcript_delta>\n</realtime_delegation>";
    let native = json!({"type":"userMessage","id":"voice","content":[{"type":"text","text":text}]});
    let images = crate::local::images::ImageStore::default();

    let entries = timeline_items(&native, "turn", "time", &images).unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].key, "native:turn:voice");
    assert_eq!(entries[0].item["text"], "能进入这个 thread 吗？");
    assert_eq!(
        entries[0].item["voiceTranscript"],
        json!([
            {"role":"user","text":"试试看"},
            {"role":"assistant","text":"我能听到你。"},
            {"role":"user","text":"能进入这个 thread 吗？"}
        ])
    );
    assert!(!entries[0].item.to_string().contains("realtime_delegation"));
    assert_eq!(
        entries,
        timeline_items(&native, "turn", "time", &images).unwrap()
    );

    let timeline = crate::storage::timeline::Timeline::memory().unwrap();
    let old = NativeItem {
        item: json!({"type":"user_message","messageId":"voice","text":text}),
        ..entries[0].clone()
    };
    timeline.append("agent", "codex", &[old]).unwrap();
    timeline.reconcile("agent", "codex", &entries).unwrap();
    assert_eq!(timeline.read("agent").unwrap().1[0].entry, entries[0]);
}
