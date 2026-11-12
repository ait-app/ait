use super::*;

#[test]
fn replies_include_the_original_link_and_preserve_routing() {
    let mut value = update();
    value["message"]["message_id"] = json!(42);
    value["message"]["reply_to_message"] = json!({
        "message_id": 41,
        "from": {"id": 7},
        "text": "https://x.com/paseo_sh/status/123",
        "reply_to_message": {"text": "older context"}
    });

    let message = incoming(&channel(), &value, 2, "ait_bot").unwrap().unwrap();

    assert_eq!(
        message.text,
        "@ait_bot hello\n\n[Quoted Telegram message]\nhttps://x.com/paseo_sh/status/123"
    );
    assert_eq!(message.reply_to.as_deref(), Some("42"));
    assert_eq!(message.thread.as_deref(), Some("3"));
    assert_eq!(message.sender, "1");
}

#[test]
fn selected_quotes_take_precedence_and_work_without_the_original_message() {
    for reply in [Value::Null, json!({"text": "full original message"})] {
        let mut value = update();
        value["message"]["quote"] = json!({"text": "selected passage", "position": 5});
        value["message"]["reply_to_message"] = reply;

        let message = incoming(&channel(), &value, 2, "ait_bot").unwrap().unwrap();

        assert_eq!(
            message.text,
            "@ait_bot hello\n\n[Quoted Telegram message]\nselected passage"
        );
    }
}

#[test]
fn empty_quotes_fall_back_to_reply_text_or_media_captions() {
    for reply in [
        json!({"text": "reference", "caption": "unused caption"}),
        json!({"caption": "reference"}),
        json!({"text": "  ", "caption": "reference"}),
    ] {
        let message = json!({"quote": {"text": " \n"}, "reply_to_message": reply});
        assert_eq!(
            text_with_reply("question", &message),
            "question\n\n[Quoted Telegram message]\nreference"
        );
    }
}

#[test]
fn missing_or_unusable_reply_content_leaves_the_request_unchanged() {
    for message in [
        json!({}),
        json!({"reply_to_message": {"photo": [{"file_id": "photo"}]}}),
        json!({"quote": {"text": "\u{0000}"}, "reply_to_message": {"text": " "}}),
    ] {
        assert_eq!(text_with_reply("question", &message), "question");
    }
}

#[test]
fn long_quotes_are_truncated_at_utf8_boundaries_within_the_input_limit() {
    let text = "@ait_bot question";
    let message = json!({"quote": {"text": "中文引用".repeat(INPUT_TEXT_LIMIT)}});

    let result = text_with_reply(text, &message);

    assert!(result.starts_with(text));
    assert!(result.contains("中文引用"));
    assert!(result.ends_with("\n[truncated]"));
    assert!(result.len() <= INPUT_TEXT_LIMIT);
    assert!(result.len() >= INPUT_TEXT_LIMIT - 3);
}

#[test]
fn requests_at_the_input_limit_are_preserved_without_appending_context() {
    let text = "x".repeat(INPUT_TEXT_LIMIT);
    let message = json!({"quote": {"text": "reference"}});

    assert_eq!(text_with_reply(&text, &message), text);
}

#[test]
fn mentions_in_quotes_do_not_trigger_group_messages() {
    let mut value = update();
    value["message"]["text"] = json!("question");
    value["message"]["quote"] = json!({"text": "@ait_bot quoted mention"});
    value["message"]["reply_to_message"] = json!({
        "from": {"id": 7}, "text": "@ait_bot original mention"
    });

    assert!(
        incoming(&channel(), &value, 2, "ait_bot")
            .unwrap()
            .is_none()
    );
}
