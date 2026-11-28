use std::path::Path;

use domain::bot::{DeliveryStatus, Turn, TurnMessage, TurnMessageStatus, TurnStatus};
use domain::{BotTurnId, ConversationId};

use super::*;

fn turn(text: &str) -> Turn {
    Turn {
        id: BotTurnId::generate(),
        conversation_id: ConversationId::generate(),
        message_id: "message".into(),
        sequence: 0,
        generation: 0,
        text: "input".into(),
        reply_to: None,
        reply: Some(text.into()),
        messages: Vec::new(),
        pending_permissions: Vec::new(),
        status: TurnStatus::Completed,
        delivery: DeliveryStatus::Pending,
        error: None,
        created_at: "2026-11-20T00:00:00Z".into(),
        wake: None,
        silent: false,
        notification_cursor: None,
        bot_sequence: None,
        sender_type: None,
        passive: false,
        context: None,
    }
}

#[test]
fn splits_provider_images_without_losing_surrounding_markdown() {
    let parts = reply_parts(&turn("before ![chart](/images/chart.png) after"));
    assert_eq!(
        parts,
        vec![
            ReplyPart::Text("before ".into()),
            ReplyPart::Image {
                alt: "chart".into(),
                source: "/images/chart.png".into(),
            },
            ReplyPart::Text(" after".into()),
        ]
    );
}

#[test]
fn image_urls_may_use_markdown_angle_brackets() {
    assert_eq!(
        reply_parts(&turn("![chart](<https://example.com/chart.png>)")),
        vec![ReplyPart::Image {
            alt: "chart".into(),
            source: "https://example.com/chart.png".into(),
        }]
    );
}

#[test]
fn malformed_image_syntax_remains_text() {
    assert_eq!(
        reply_parts(&turn("![missing(/image.png")),
        vec![ReplyPart::Text("![missing(/image.png".into())]
    );
}

#[test]
fn local_sources_stay_inside_the_provider_image_directory() {
    assert!(local_path("/tmp/image.png").unwrap().is_absolute());
    assert!(mime_from_path(Path::new("image.jpg")).is_some());
    assert!(mime_from_path(Path::new("image.txt")).is_none());
}

#[test]
fn data_urls_are_decoded_with_a_supported_mime_type() {
    let image = decode_data_url("image/png;base64,aGVsbG8=").unwrap();
    assert_eq!(image.bytes, b"hello");
    assert_eq!(image.mime_type, "image/png");
    assert_eq!(image.file_name, "image.png");
    assert!(decode_data_url("text/plain;base64,aGVsbG8=").is_err());
}

#[test]
fn image_reply_prefers_distinct_streamed_messages() {
    let mut turn = turn("legacy");
    turn.messages = vec![TurnMessage {
        id: "answer".into(),
        text: "![Image](https://example.com/image.png)".into(),
        status: TurnMessageStatus::Completed,
    }];
    assert_eq!(
        reply_parts(&turn),
        vec![ReplyPart::Image {
            alt: "Image".into(),
            source: "https://example.com/image.png".into(),
        }]
    );
}

#[test]
fn image_data_rejects_empty_and_unsupported_content() {
    assert!(image_data(Vec::new(), Some("image/png"), "image".into()).is_err());
    assert!(image_data(b"x".to_vec(), None, "image".into()).is_err());
}
