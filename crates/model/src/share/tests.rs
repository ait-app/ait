use serde_json::json;

use super::*;

fn visitor() -> Visitor {
    Visitor {
        user_id: Uuid::nil(),
        display_name: Some("访客 Bob".into()),
        organization_id: Uuid::max(),
        share_id: Uuid::nil(),
        workspace_key: "ws_1".into(),
        permission: "operate".into(),
    }
}

#[test]
fn visitors_round_trip_through_an_ascii_header() {
    let encoded = visitor().encode();
    assert!(
        encoded
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    );
    assert_eq!(Visitor::decode(&encoded), Some(visitor()));
}

#[test]
fn malformed_headers_are_rejected() {
    assert_eq!(Visitor::decode("not base64!"), None);
    assert_eq!(Visitor::decode(&URL_SAFE_NO_PAD.encode(b"{}")), None);
}

#[test]
fn published_entries_match_the_center_wire_form() {
    let entry = Published {
        workspace_key: "ws_1".into(),
        organization_id: Uuid::nil(),
        permission: "operate".into(),
        audience: Audience::Members,
        recipients: vec![Uuid::max()],
        display_name: None,
        project_name: None,
        generation: Some(2),
    };
    assert_eq!(
        serde_json::to_value(&entry).unwrap(),
        json!({"workspaceKey":"ws_1","organizationId":Uuid::nil(),"permission":"operate",
            "audience":"members","recipients":[Uuid::max()],"generation":2})
    );
}

#[test]
fn outcomes_parse_replies_and_status_frames() {
    let reply: Outcome = serde_json::from_value(json!({
        "accepted":[{"shareId":Uuid::nil(),"workspaceKey":"ws_1","organizationId":Uuid::nil(),"generation":1}],
        "rejected":[{"workspaceKey":"ws_2","organizationId":Uuid::nil(),"code":"revoked"}],
        "orgs":[{"organizationId":Uuid::nil(),"name":"Acme","status":"active","myRole":"member"}]
    }))
    .unwrap();
    assert_eq!(reply.revision, None);
    assert_eq!(reply.accepted[0].generation, 1);
    assert_eq!(reply.rejected[0].code, "revoked");
    let status: Outcome =
        serde_json::from_value(json!({"revision":4,"accepted":[],"rejected":[],"orgs":[]}))
            .unwrap();
    assert_eq!(status.revision, Some(4));
}
