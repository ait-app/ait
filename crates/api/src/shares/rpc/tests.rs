use serde_json::json;
use uuid::Uuid;

use crate::connection::rpc::execute;
use crate::test_support::{Workspaces, address};

#[tokio::test]
async fn owner_methods_validate_and_report_settings() {
    let fixture = Workspaces::new(address());
    let state = &fixture.api.shared;
    let organization = fixture.organization;
    let share = |workspace: &str| {
        json!({
            "workspaceKey": workspace,
            "organizationId": organization,
            "permission": "operate",
            "audience": "organization",
        })
    };
    let stored = execute(state, "share.set.request", share(&fixture.shared))
        .await
        .unwrap();
    assert_eq!(stored["share"]["workspaceKey"], json!(fixture.shared));
    assert_eq!(stored["share"]["missingRecipients"], json!([]));
    assert_eq!(
        execute(state, "share.set.request", share("missing")).await,
        Err(model::ErrorCode::WorkspaceNotFound)
    );
    let mut generation = share(&fixture.shared);
    generation["generation"] = json!(1);
    assert_eq!(
        execute(state, "share.set.request", generation).await,
        Err(model::ErrorCode::InvalidMessage)
    );
    fixture.archive(&fixture.other);
    assert_eq!(
        execute(state, "share.set.request", share(&fixture.other)).await,
        Err(model::ErrorCode::WorkspaceNotFound)
    );
    let listed = execute(state, "share.list.request", json!({}))
        .await
        .unwrap();
    assert_eq!(listed["shares"].as_array().unwrap().len(), 1);
    assert_eq!(
        execute(state, "share.orgs.list.request", json!({}))
            .await
            .unwrap(),
        json!({"orgs": []})
    );
    let removed = execute(
        state,
        "share.remove.request",
        json!({"workspaceKey": fixture.shared, "organizationId": organization}),
    )
    .await
    .unwrap();
    assert_eq!(removed["removed"], true);
    assert_eq!(
        execute(
            state,
            "share.remove.request",
            json!({"workspaceKey": fixture.shared, "organizationId": Uuid::nil(), "x": 1}),
        )
        .await,
        Err(model::ErrorCode::InvalidMessage)
    );
}

#[tokio::test]
async fn archiving_a_workspace_removes_its_shares() {
    let fixture = Workspaces::new(address());
    let visitor = fixture.accept_share().await;
    let registration = fixture.api.shared.shares.admit(&visitor).unwrap();
    let result = execute(
        &fixture.api.shared,
        "workspace.archive.request",
        json!({"workspaceId": fixture.shared}),
    )
    .await
    .unwrap();
    assert!(result["archivedAt"].is_string());
    assert!(registration.cancellation().is_cancelled());
    assert_eq!(fixture.api.share_book().publication().1, Vec::new());
}
