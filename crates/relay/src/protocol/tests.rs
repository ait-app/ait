use serde_json::{Value, json};

use super::*;

#[test]
fn download_messages_keep_their_wire_shapes() {
    for (message, expected) in [
        (
            DownloadMessage::Headers {
                status: 200,
                content_length: Some(42),
                content_type: Some("application/octet-stream"),
            },
            json!({"type":"download.headers","status":200,"content_length":42,
                "content_type":"application/octet-stream"}),
        ),
        (
            DownloadMessage::Headers {
                status: 200,
                content_length: None,
                content_type: None,
            },
            json!({"type":"download.headers","status":200,"content_length":null,"content_type":null}),
        ),
        (
            DownloadMessage::End { bytes: 42 },
            json!({"type":"download.end","bytes":42}),
        ),
    ] {
        assert_eq!(
            serde_json::from_str::<Value>(&encode(&message).unwrap()).unwrap(),
            expected
        );
    }
}

#[test]
fn handshake_messages_reject_wrong_kinds() {
    assert!(decode::<ClientHello>(r#"{"type":"hello","future":{"extension":true}}"#).is_ok());
    assert!(decode::<ClientHello>(r#"{"type":"request"}"#).is_err());
    assert!(decode::<DownloadAck>(r#"{"type":"download.complete","future":true}"#).is_ok());
    assert!(decode::<DownloadAck>(r#"{"type":"download.end"}"#).is_err());
}

#[test]
fn download_token_syntax_stays_bounded() {
    for token in ["abc-123".to_owned(), "x".repeat(128)] {
        assert!(validate_download_token(&token).is_ok());
    }
    for token in [
        String::new(),
        "x".repeat(129),
        "a/b".to_owned(),
        "a?b".to_owned(),
        "a\nb".to_owned(),
        "中文".to_owned(),
    ] {
        assert!(matches!(
            validate_download_token(&token),
            Err(Error::Protocol)
        ));
    }
}

#[test]
fn the_retired_relay_status_is_always_offline() {
    assert_eq!(
        serde_json::to_value(Status::default()).unwrap(),
        json!({"connecting":false,"online":false,"epoch":null,"error":null})
    );
}
