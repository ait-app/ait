use super::*;
use serde_json::json;

#[test]
fn readable_card_and_post_preserve_order_without_actions() {
    let card = json!({"title":"Service", "elements":[[{"tag":"text","text":"Unavailable"},{"tag":"a","text":"Logs","href":"https://example.com"}]],"value":{"text":"do not run"}});
    assert_eq!(
        text("interactive", &card).unwrap(),
        "Service\nUnavailable\nLogs\nhttps://example.com"
    );
    assert!(text("image", &json!({"image_key":"secret"})).is_none());
    let post = json!({"title":"Report","content":[[{"tag":"text","text":"Ready"}]]});
    assert_eq!(text("post", &post).unwrap(), "Report\nReady");
}

#[test]
fn truncation_respects_utf8_and_input_limit() {
    let extracted = text("text", &json!({"text":"测".repeat(10_000)})).unwrap();
    assert!(extracted.len() < 16 * 1024);
    assert!(extracted.ends_with("[Message truncated]"));
}
