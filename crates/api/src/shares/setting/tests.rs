use serde_json::json;

use super::*;

fn request(value: serde_json::Value) -> SetRequest {
    serde_json::from_value(value).unwrap()
}

fn members(recipients: &[Uuid]) -> SetRequest {
    request(json!({
        "workspaceKey": "w",
        "organizationId": Uuid::nil(),
        "permission": OPERATE,
        "audience": "members",
        "recipients": recipients,
    }))
}

#[test]
fn normalizes_recipients_and_names() {
    let first = Uuid::from_u128(2);
    let second = Uuid::from_u128(1);
    let mut set = members(&[first, second, first]);
    set.display_name = Some("  Demo  ".into());
    set.project_name = Some("   ".into());
    let set = set.normalize().unwrap();
    assert_eq!(set.recipients, vec![second, first]);
    assert_eq!(set.display_name.as_deref(), Some("Demo"));
    assert_eq!(set.project_name, None);
}

#[test]
fn rejects_invalid_requests() {
    let mut permission = members(&[Uuid::nil()]);
    permission.permission = "read".into();
    assert_eq!(permission.normalize(), Err(ShareError::Permission));
    assert_eq!(members(&[]).normalize(), Err(ShareError::Audience));
    let mut organization = members(&[Uuid::nil()]);
    organization.audience = Audience::Organization;
    assert_eq!(organization.normalize(), Err(ShareError::Audience));
    let many: Vec<Uuid> = (0..=MAX_RECIPIENTS as u128).map(Uuid::from_u128).collect();
    assert_eq!(members(&many).normalize(), Err(ShareError::Recipients));
    let mut long = members(&[Uuid::nil()]);
    long.display_name = Some("x".repeat(201));
    assert_eq!(long.normalize(), Err(ShareError::Name));
    let mut control = members(&[Uuid::nil()]);
    control.project_name = Some("a\nb".into());
    assert_eq!(control.normalize(), Err(ShareError::Name));
    let mut blank = members(&[Uuid::nil()]);
    blank.workspace_key = " ".into();
    assert_eq!(blank.normalize(), Err(ShareError::Workspace));
}

#[test]
fn generation_cannot_be_set_by_the_owner() {
    let result = serde_json::from_value::<SetRequest>(json!({
        "workspaceKey": "w",
        "organizationId": Uuid::nil(),
        "permission": OPERATE,
        "audience": "organization",
        "generation": 3,
    }));
    assert!(result.is_err());
}

#[test]
fn update_reports_changes_and_retries_rejected_entries() {
    let user = Uuid::from_u128(7);
    let mut setting = members(&[user]).normalize().unwrap().into_setting();
    assert_eq!(setting.state, ShareState::Pending);
    assert_eq!(setting.generation, None);
    assert!(!members(&[user]).normalize().unwrap().update(&mut setting));
    setting.reject("no_recipients");
    let other = Uuid::from_u128(8);
    assert!(members(&[other]).normalize().unwrap().update(&mut setting));
    assert_eq!(setting.state, ShareState::Pending);
    assert_eq!(setting.code, None);
    assert_eq!(setting.recipients, vec![other]);
}

#[test]
fn only_accepted_matching_shares_admit_visitors() {
    let user = Uuid::from_u128(7);
    let share_id = Uuid::from_u128(9);
    let mut setting = members(&[user]).normalize().unwrap().into_setting();
    let visitor = Visitor {
        user_id: user,
        display_name: None,
        organization_id: Uuid::nil(),
        share_id,
        workspace_key: "w".into(),
        permission: OPERATE.into(),
    };
    assert!(!setting.admits(&visitor));
    setting.accept(&Accepted {
        share_id,
        workspace_key: "w".into(),
        organization_id: Uuid::nil(),
        generation: 4,
        missing_recipients: vec![user],
    });
    assert!(setting.admits(&visitor));
    assert_eq!(setting.published().generation, Some(4));
    assert_eq!(setting.missing_recipients, vec![user]);
    let stranger = Visitor {
        user_id: Uuid::from_u128(1),
        ..visitor.clone()
    };
    assert!(!setting.admits(&stranger));
    let wrong_share = Visitor {
        share_id: Uuid::from_u128(1),
        ..visitor.clone()
    };
    assert!(!setting.admits(&wrong_share));
    let other_workspace = Visitor {
        workspace_key: "x".into(),
        ..visitor.clone()
    };
    assert!(!setting.admits(&other_workspace));
    setting.reject("org_inactive");
    assert!(!setting.admits(&visitor));
    assert_eq!(setting.share_id, Some(share_id));
}

#[test]
fn errors_map_to_safe_codes() {
    assert_eq!(
        ErrorCode::from(ShareError::Capacity),
        ErrorCode::ResourceExhausted
    );
    assert_eq!(
        ErrorCode::from(ShareError::Workspace),
        ErrorCode::WorkspaceNotFound
    );
    assert_eq!(
        ErrorCode::from(ShareError::Audience),
        ErrorCode::InvalidMessage
    );
}
