use std::collections::BTreeSet;

use super::*;

#[test]
fn catalog_and_implemented_methods_keep_their_envelope_metadata() {
    let implemented: BTreeSet<_> = implemented_methods().collect();
    assert_eq!(implemented.len(), 179);
    for method in &implemented {
        let metadata = lookup(method).expect("implemented method must exist");
        let kind = PASEO_METHODS
            .iter()
            .find(|spec| spec.canonical_name == *method)
            .map_or(InboundKind::Request, |spec| spec.kind);
        assert_eq!(metadata.kind, kind, "{method}");
        assert_eq!(metadata.capability, *method);
    }
    for spec in PASEO_METHODS {
        assert_eq!(lookup(spec.canonical_name).unwrap().kind, spec.kind);
    }
    assert!(lookup("connection.single.v1").is_none());
    for unknown in [
        "project.list.unknown",
        "schedule/list",
        "register_push_token",
    ] {
        assert!(lookup(unknown).is_none());
    }
}

#[test]
fn request_validation_preserves_error_precedence_and_status_unsubscribe_alias() {
    let implemented = ["connection.ping".to_owned()];
    let negotiated = [
        "connection.ping".to_owned(),
        "server.status.subscribe".to_owned(),
    ];
    assert_eq!(
        request("connection.ping", &implemented, &negotiated),
        Ok(())
    );
    assert_eq!(
        request("server.status.unsubscribe", &implemented, &negotiated),
        Ok(())
    );
    assert_eq!(
        request("server.status.unsubscribe", &implemented, &[]),
        Err(ErrorCode::UnsupportedCapability)
    );
    assert_eq!(request("unknown", &[], &[]), Err(ErrorCode::MethodNotFound));
    assert_eq!(
        request("terminal.input", &[], &[]),
        Err(ErrorCode::InvalidMessage)
    );
    assert_eq!(
        request("schedule.list.request", &[], &[]),
        Err(ErrorCode::UnsupportedCapability)
    );
    assert_eq!(
        request(
            "schedule.list.request",
            &[],
            &["schedule.list.request".to_owned()]
        ),
        Err(ErrorCode::NotImplemented)
    );
}

#[test]
fn placeholders_keep_direction_and_negotiation_checks() {
    let negotiated = ["terminal.input".to_owned()];
    assert_eq!(
        placeholder("terminal.input", InboundKind::Event, &negotiated),
        ErrorCode::NotImplemented
    );
    assert_eq!(
        placeholder("terminal.input", InboundKind::Response, &negotiated),
        ErrorCode::InvalidMessage
    );
    assert_eq!(
        placeholder("push.register", InboundKind::Event, &negotiated),
        ErrorCode::UnsupportedCapability
    );
    assert_eq!(
        placeholder("register_push_token", InboundKind::Event, &negotiated),
        ErrorCode::MethodNotFound
    );
}
