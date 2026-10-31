use serde_json::json;

use super::*;

fn visitor() -> Visitor {
    Visitor {
        user_id: Uuid::nil(),
        display_name: Some("访客 Bob".into()),
        username: Some("bob".into()),
        group_id: Uuid::max(),
        share_id: Uuid::nil(),
        resource_kind: ResourceKind::Project,
        resource_key: "prj_1".into(),
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
fn visitors_use_the_flat_snake_case_wire_form() {
    let value = serde_json::to_value(visitor()).unwrap();
    assert_eq!(
        value,
        json!({"user_id":Uuid::nil(),"display_name":"访客 Bob","username":"bob",
            "group_id":Uuid::max(),"share_id":Uuid::nil(),"resource_kind":"project",
            "resource_key":"prj_1","permission":"operate"})
    );
    let node: Visitor = serde_json::from_value(json!({"user_id":Uuid::nil(),
        "display_name":null,"group_id":Uuid::nil(),"share_id":Uuid::nil(),
        "resource_kind":"node","resource_key":"","permission":"operate"}))
    .unwrap();
    assert_eq!(node.scope(), Some(ShareScope::Node));
    // Older centers and accounts without a handle send no username.
    assert_eq!(node.username, None);
    assert!(
        serde_json::to_value(&node)
            .unwrap()
            .get("username")
            .is_none()
    );
}

#[test]
fn malformed_headers_are_rejected() {
    assert_eq!(Visitor::decode("not base64!"), None);
    assert_eq!(Visitor::decode(&URL_SAFE_NO_PAD.encode(b"{}")), None);
    let mut unknown = serde_json::to_value(visitor()).unwrap();
    unknown["resource_kind"] = json!("organization");
    assert_eq!(
        Visitor::decode(&URL_SAFE_NO_PAD.encode(unknown.to_string())),
        None
    );
}

#[test]
fn scopes_require_keys_that_match_their_kind() {
    assert_eq!(
        ShareScope::new(ResourceKind::Workspace, "ws_1"),
        Some(ShareScope::Workspace("ws_1".into()))
    );
    assert_eq!(
        ShareScope::new(ResourceKind::Project, "prj_1"),
        Some(ShareScope::Project("prj_1".into()))
    );
    assert_eq!(
        ShareScope::new(ResourceKind::Node, ""),
        Some(ShareScope::Node)
    );
    assert_eq!(ShareScope::new(ResourceKind::Node, "ws_1"), None);
    assert_eq!(ShareScope::new(ResourceKind::Workspace, " "), None);
    assert_eq!(ShareScope::new(ResourceKind::Project, ""), None);
    for scope in [
        ShareScope::Workspace("ws_1".into()),
        ShareScope::Project("prj_1".into()),
        ShareScope::Node,
    ] {
        assert_eq!(ShareScope::new(scope.kind(), scope.key()), Some(scope));
    }
    assert_eq!(visitor().scope(), Some(ShareScope::Project("prj_1".into())));
}

#[test]
fn published_entries_match_the_center_wire_form() {
    let entry = Published {
        resource_kind: ResourceKind::Workspace,
        resource_key: "ws_1".into(),
        group_id: Uuid::nil(),
        permission: "operate".into(),
        audience: Audience::Members,
        recipients: vec![Uuid::max()],
        display_name: None,
        project_name: None,
        setting_id: Uuid::max(),
    };
    assert_eq!(
        serde_json::to_value(&entry).unwrap(),
        json!({"resourceKind":"workspace","resourceKey":"ws_1","groupId":Uuid::nil(),
            "permission":"operate","audience":"members","recipients":[Uuid::max()],
            "settingId":Uuid::max()})
    );
    let node = Published {
        resource_kind: ResourceKind::Node,
        resource_key: String::new(),
        audience: Audience::Group,
        recipients: Vec::new(),
        ..entry
    };
    assert_eq!(
        serde_json::to_value(&node).unwrap(),
        json!({"resourceKind":"node","resourceKey":"","groupId":Uuid::nil(),
            "permission":"operate","audience":"group","settingId":Uuid::max()})
    );
}

#[test]
fn outcomes_parse_replies_and_status_frames() {
    let reply: Outcome = serde_json::from_value(json!({
        "revision":3,
        "accepted":[{"shareId":Uuid::nil(),"resourceKind":"project","resourceKey":"prj_1",
            "groupId":Uuid::nil(),"settingId":Uuid::max(),"missingRecipients":[Uuid::max()]}],
        "rejected":[{"resourceKind":"workspace","resourceKey":"ws_2","groupId":Uuid::nil(),
            "settingId":Uuid::nil(),"code":"group_inactive"}],
        "denied":[{"shareId":Uuid::nil(),"userId":Uuid::max()}],
        "groups":[{"groupId":Uuid::nil(),"name":"Acme","status":"active","myRole":"member"}]
    }))
    .unwrap();
    assert_eq!(reply.revision, 3);
    assert_eq!(reply.accepted[0].setting_id, Uuid::max());
    assert_eq!(reply.rejected[0].setting_id, Uuid::nil());
    assert_eq!(
        reply.denied,
        vec![VisitorGrant {
            share_id: Uuid::nil(),
            user_id: Uuid::max()
        }]
    );
    assert_eq!(reply.accepted[0].resource_kind, ResourceKind::Project);
    assert_eq!(reply.accepted[0].missing_recipients, vec![Uuid::max()]);
    assert_eq!(reply.rejected[0].code, "group_inactive");
    assert_eq!(reply.groups[0].name, "Acme");
    let status: Outcome =
        serde_json::from_value(json!({"revision":4,"accepted":[],"rejected":[],"groups":[]}))
            .unwrap();
    assert_eq!(status.revision, 4);
    assert!(status.denied.is_empty());
}
