use serde_json::json;

use super::*;

fn header(id: &str) -> String {
    json!({"type":"session","version":3,"id":id,"cwd":"/work",
        "timestamp":"2026-10-21T00:00:00Z"})
    .to_string()
}

fn user(id: &str, text: &str, time: &str) -> String {
    json!({"type":"message","id":id,"parentId":null,"timestamp":time,
        "message":{"role":"user","timestamp":1000,"content":[{"type":"text","text":text}]}})
    .to_string()
}

fn describe_lines(name: &str, lines: &[String]) -> Option<SessionDescriptor> {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(name);
    std::fs::write(&path, lines.join("\n") + "\n").unwrap();
    describe(&path, |_| true).unwrap().descriptor
}

const DATED: &str = "2026-10-21T00-00-00-000Z_01a1ca98-b59b-724c-916c-215b8f600344.jsonl";

#[test]
fn titles_prefer_native_names_then_readable_file_names_then_the_first_prompt() {
    let title = |extra: &[String], name: &str| {
        let mut lines = vec![
            header("id"),
            user("u", "first prompt", "2026-10-21T00:00:01Z"),
        ];
        lines.extend_from_slice(extra);
        describe_lines(name, &lines).unwrap().title
    };
    let slot = |title: &str| json!({"type":"title","v":1,"title":title}).to_string();
    let info = |name: &str| json!({"type":"session_info","id":"i","name":name}).to_string();

    assert_eq!(title(&[slot("Slot")], DATED).as_deref(), Some("Slot"));
    // The tail is read from its end: the latest title or Pi `session_info` name wins.
    assert_eq!(
        title(&[slot("Slot"), info("Info")], DATED).as_deref(),
        Some("Info")
    );
    assert_eq!(
        title(&[slot("")], "Prober.jsonl").as_deref(),
        Some("Prober")
    );
    assert_eq!(title(&[slot("  ")], DATED).as_deref(), Some("first prompt"));
    assert_eq!(
        title(&[], "0123abcdef.jsonl").as_deref(),
        Some("first prompt")
    );
}

#[test]
fn previews_are_capped_and_activity_is_the_last_message_time() {
    let long = "word ".repeat(100);
    let lines = [
        header("id"),
        user("u1", &long, "2026-10-21T00:00:01Z"),
        user("u2", "  last   prompt ", "2026-10-21T01:00:00+01:00"),
        json!({"type":"model_usage","id":"x","timestamp":"2026-10-22T00:00:00Z"}).to_string(),
    ];

    let descriptor = describe_lines(DATED, &lines).unwrap();

    let first = descriptor.first_prompt_preview.unwrap();
    assert_eq!(first.chars().count(), 160);
    assert_eq!(
        descriptor.last_prompt_preview.as_deref(),
        Some("last prompt")
    );
    assert_eq!(descriptor.last_activity_at, "2026-10-21T00:00:00Z");
    assert_eq!(descriptor.provider_handle_id, "id");
}

#[test]
fn activity_falls_back_to_any_entry_time_then_the_file_time() {
    let custom = json!({"type":"custom_message","id":"c","customType":"note","content":"x",
        "timestamp":"2026-10-23T00:00:00Z"})
    .to_string();
    let descriptor = describe_lines(DATED, &[header("id"), custom.clone()]).unwrap();
    assert_eq!(descriptor.last_activity_at, "2026-10-23T00:00:00Z");

    // The header's time counts as an entry time, as in Paseo; without any, the file's.
    let untimed =
        json!({"type":"custom_message","id":"c","customType":"note","content":"x"}).to_string();
    let descriptor = describe_lines(DATED, &[header("id"), untimed.clone()]).unwrap();
    assert_eq!(descriptor.last_activity_at, "2026-10-21T00:00:00Z");
    let header = json!({"type":"session","version":3,"id":"id","cwd":"/work"}).to_string();
    let descriptor = describe_lines(DATED, &[header, untimed]).unwrap();
    let modified = DateTime::parse_from_rfc3339(&descriptor.last_activity_at).unwrap();
    assert!((Utc::now() - modified.with_timezone(&Utc)).num_minutes() < 5);
}

#[test]
fn files_without_a_header_a_conversation_or_the_workspace_are_not_listed() {
    let model = json!({"type":"model_change","id":"m","model":"test/model"}).to_string();
    assert!(describe_lines(DATED, &[header("id"), model]).is_none());
    assert!(describe_lines(DATED, &[user("u", "hello", "2026-10-21T00:00:01Z")]).is_none());

    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(DATED);
    std::fs::write(
        &path,
        [header("id"), user("u", "hello", "2026-10-21T00:00:01Z")].join("\n"),
    )
    .unwrap();
    let skipped = describe(&path, |cwd| cwd == "/elsewhere").unwrap();
    assert!(skipped.descriptor.is_none());
    assert_eq!(skipped.bytes, std::fs::metadata(&path).unwrap().len());
    assert!(describe(&root.path().join("missing.jsonl"), |_| true).is_none());
}

#[test]
fn large_files_are_described_from_a_bounded_head_and_tail() {
    let filler = json!({"type":"model_usage","id":"x","data":"x".repeat(1024)}).to_string();
    let mut lines = vec![
        header("id"),
        user("u1", "opening prompt", "2026-10-21T00:00:01Z"),
    ];
    lines.extend(std::iter::repeat_n(filler, 512));
    lines.push(user("u2", "closing prompt", "2026-10-21T05:00:00Z"));
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(DATED);
    std::fs::write(&path, lines.join("\n")).unwrap();

    let listed = describe(&path, |_| true).unwrap();

    let descriptor = listed.descriptor.unwrap();
    assert_eq!(
        descriptor.first_prompt_preview.as_deref(),
        Some("opening prompt")
    );
    assert_eq!(
        descriptor.last_prompt_preview.as_deref(),
        Some("closing prompt")
    );
    assert_eq!(descriptor.last_activity_at, "2026-10-21T05:00:00Z");
    assert_eq!(listed.bytes, HEAD_BYTES + TAIL_BYTES);
}

#[test]
fn only_undated_non_hexadecimal_file_names_read_as_titles() {
    for (name, expected) in [
        (DATED, None),
        ("2026-10-21T00:00:00_x.jsonl", None),
        ("01a1ca98b59b.jsonl", None),
        ("Prober.jsonl", Some("Prober")),
        ("2026-10-21_session.jsonl", Some("2026-10-21_session")),
        ("abc.jsonl", Some("abc")),
    ] {
        assert_eq!(
            readable_name(Path::new(name)).as_deref(),
            expected,
            "{name}"
        );
    }
}
