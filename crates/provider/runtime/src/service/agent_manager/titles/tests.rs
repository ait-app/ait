use super::*;
use serde_json::json;

#[test]
fn prompt_titles_collapse_whitespace_and_respect_utf16_limits() {
    assert_eq!(from_prompt(" \n\t\0 "), None);
    assert_eq!(
        from_prompt("  修复\n session\t标题\0 "),
        Some("修复 session 标题".to_owned())
    );
    assert_eq!(from_prompt(&"x".repeat(201)), Some("x".repeat(200)));
    assert_eq!(from_prompt(&"😀".repeat(101)), Some("😀".repeat(100)));
    assert_eq!(
        from_prompt(&format!("{} 😀", "x".repeat(198))),
        Some("x".repeat(198))
    );
}

#[test]
fn only_nonempty_user_messages_supply_history_titles() {
    let entries: Vec<_> = [
        json!({"type":"assistant_message","text":"Assistant response"}),
        json!({"type":"user_message","text":" \n "}),
        json!({"type":"user_message","attachments":[]}),
        json!({"type":"user_message","text":" First question "}),
        json!({"type":"user_message","text":"Second question"}),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, item)| NativeItem {
        key: index.to_string(),
        turn_id: None,
        timestamp: "2026-09-27T00:00:00Z".to_owned(),
        item,
    })
    .collect();
    assert_eq!(from_entries(&entries), Some("First question".to_owned()));
    assert_eq!(from_entries(&entries[..3]), None);
}
