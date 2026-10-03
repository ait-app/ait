use secrecy::ExposeSecret;
use serde_json::{Value, json};

use super::*;

#[test]
fn control_hello_and_download_messages_keep_their_wire_shapes() {
    let id = Uuid::nil();
    let hello = ControlHello::new(id, "server", "instance", None);
    assert_eq!(
        serde_json::from_str::<Value>(&encode(&hello).unwrap()).unwrap(),
        json!({"type":"control.hello","version":1,"node_session_id":id,
            "server_id":"server","instance_id":"instance","resume_epoch":null})
    );
    let resumed = ControlHello::new(id, "server", "instance", Some(id));
    assert_eq!(
        serde_json::to_value(resumed).unwrap()["resume_epoch"],
        id.to_string()
    );
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
fn control_commands_require_mode_specific_fields_and_hide_credentials() {
    let base = json!({"type":"open_data","relay_session_id":Uuid::nil(),
        "epoch":Uuid::nil(),"daemon_ticket":"private-ticket","mode":"ait-rust-single-v1"});
    let ControlCommand::OpenData(grant) = decode(&base.to_string()).unwrap() else {
        panic!("expected open data");
    };
    assert!(matches!(grant.mode, DataMode::RustSingle));
    assert_eq!(grant.daemon_ticket.expose_secret(), "private-ticket");
    assert!(!format!("{grant:?}").contains("private-ticket"));
    let mut download = base.clone();
    download["mode"] = json!("ait-download-v1");
    assert!(decode::<ControlCommand>(&download.to_string()).is_err());
    download["download_token"] = json!("private-download-token");
    let command: ControlCommand = decode(&download.to_string()).unwrap();
    assert!(!format!("{command:?}").contains("private-download-token"));
    let ControlCommand::OpenData(OpenData {
        mode: DataMode::Download { download_token },
        ..
    }) = command
    else {
        panic!("expected download grant");
    };
    assert_eq!(download_token.expose_secret(), "private-download-token");
    for (field, value) in [
        ("type", json!("future-command")),
        ("mode", json!("future-mode")),
        ("epoch", json!("invalid-uuid")),
        ("relay_session_id", json!(null)),
        ("daemon_ticket", json!(17)),
    ] {
        let mut invalid = base.clone();
        invalid[field] = value;
        assert!(matches!(
            decode::<ControlCommand>(&invalid.to_string()),
            Err(Error::Protocol)
        ));
    }
    assert!(decode::<ControlCommand>("not-json").is_err());
    let cancel = json!({"type":"cancel_session","relay_session_id":Uuid::nil(),"future":true});
    assert!(
        matches!(decode::<ControlCommand>(&cancel.to_string()).unwrap(),
        ControlCommand::CancelSession { relay_session_id } if relay_session_id == Uuid::nil())
    );
}

#[test]
fn pairing_and_handshake_messages_reject_wrong_kinds_or_sessions() {
    let id = Uuid::nil();
    let ready = json!({"type":"relay.ready","relay_session_id":id});
    decode::<Pairing>(&ready.to_string())
        .unwrap()
        .verify(id)
        .unwrap();
    assert!(
        decode::<Pairing>(&ready.to_string())
            .unwrap()
            .verify(Uuid::new_v4())
            .is_err()
    );
    assert!(decode::<Pairing>(r#"{"type":"relay.ready"}"#).is_err());
    let welcome = json!({"type":"control.welcome","epoch":id,"future":true});
    assert!(
        matches!(decode::<ControlWelcome>(&welcome.to_string()).unwrap(),
        ControlWelcome::Welcome { epoch } if epoch == id)
    );
    assert!(decode::<ControlWelcome>(&ready.to_string()).is_err());
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
