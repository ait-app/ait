use model::ErrorCode;
use model::share::ResourceKind;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::connection::rpc::execute;
use crate::test_support::{Workspaces, address};

fn share(fixture: &Workspaces, kind: &str, key: &str) -> Value {
    json!({
        "resourceKind": kind,
        "resourceKey": key,
        "groupId": fixture.group,
        "permission": "operate",
        "audience": "group",
    })
}

#[tokio::test]
async fn owner_methods_validate_and_report_settings() {
    let fixture = Workspaces::new(address());
    let state = &fixture.api.shared;
    let stored = execute(
        state,
        "share.set.request",
        share(&fixture, "workspace", &fixture.shared),
    )
    .await
    .unwrap();
    assert_eq!(stored["share"]["resourceKind"], "workspace");
    assert_eq!(stored["share"]["resourceKey"], json!(fixture.shared));
    assert_eq!(stored["share"]["groupId"], json!(fixture.group));
    assert_eq!(stored["share"]["missingRecipients"], json!([]));
    assert_eq!(
        execute(
            state,
            "share.set.request",
            share(&fixture, "workspace", "missing")
        )
        .await,
        Err(ErrorCode::WorkspaceNotFound)
    );
    let mut chosen = share(&fixture, "workspace", &fixture.shared);
    chosen["settingId"] = json!(Uuid::nil());
    assert_eq!(
        execute(state, "share.set.request", chosen).await,
        Err(ErrorCode::InvalidMessage)
    );
    fixture.archive(&fixture.other);
    assert_eq!(
        execute(
            state,
            "share.set.request",
            share(&fixture, "workspace", &fixture.other)
        )
        .await,
        Err(ErrorCode::WorkspaceNotFound)
    );
    let listed = execute(state, "share.list.request", json!({}))
        .await
        .unwrap();
    assert_eq!(listed["shares"].as_array().unwrap().len(), 1);
    assert_eq!(listed["groups"], json!([]));
    assert_eq!(
        execute(state, "share.groups.list.request", json!({}))
            .await
            .unwrap(),
        json!({"groups": []})
    );
    let removed = execute(
        state,
        "share.remove.request",
        json!({"resourceKind": "workspace", "resourceKey": fixture.shared,
            "groupId": fixture.group}),
    )
    .await
    .unwrap();
    assert_eq!(removed["removed"], true);
    assert_eq!(
        execute(
            state,
            "share.remove.request",
            json!({"resourceKind": "workspace", "resourceKey": fixture.shared,
                "groupId": Uuid::nil(), "x": 1}),
        )
        .await,
        Err(ErrorCode::InvalidMessage)
    );
}

#[tokio::test]
async fn set_validates_each_resource_kind() {
    let fixture = Workspaces::new(address());
    let state = &fixture.api.shared;
    let project = execute(
        state,
        "share.set.request",
        share(&fixture, "project", &fixture.project),
    )
    .await
    .unwrap();
    assert_eq!(project["share"]["resourceKind"], "project");
    let node = execute(state, "share.set.request", share(&fixture, "node", ""))
        .await
        .unwrap();
    assert_eq!(node["share"]["resourceKey"], "");
    for (params, code) in [
        (
            share(&fixture, "project", "prj_missing"),
            ErrorCode::WorkspaceNotFound,
        ),
        (
            share(&fixture, "project", &fixture.shared),
            ErrorCode::WorkspaceNotFound,
        ),
        (
            share(&fixture, "workspace", &fixture.project),
            ErrorCode::WorkspaceNotFound,
        ),
        (
            share(&fixture, "node", &fixture.shared),
            ErrorCode::InvalidMessage,
        ),
        (share(&fixture, "project", ""), ErrorCode::InvalidMessage),
        (
            share(&fixture, "organization", &fixture.project),
            ErrorCode::InvalidMessage,
        ),
    ] {
        assert_eq!(
            execute(state, "share.set.request", params.clone()).await,
            Err(code),
            "{params}"
        );
    }
    let listed = execute(state, "share.list.request", json!({}))
        .await
        .unwrap();
    assert_eq!(listed["shares"].as_array().unwrap().len(), 2);
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
    assert_eq!(fixture.api.share_book().publication().shares, Vec::new());
}

#[tokio::test]
async fn removing_a_project_removes_its_project_and_workspace_shares() {
    let fixture = Workspaces::new(address());
    let project = fixture
        .accept(ResourceKind::Project, &fixture.project)
        .await;
    fixture
        .accept(ResourceKind::Workspace, &fixture.sibling)
        .await;
    let other = fixture
        .accept(ResourceKind::Workspace, &fixture.other)
        .await;
    let registration = fixture.api.shared.shares.admit(&project).unwrap();
    let result = execute(
        &fixture.api.shared,
        "project.remove.request",
        json!({"projectId": fixture.project}),
    )
    .await
    .unwrap();
    assert_eq!(result["accepted"], true);
    assert!(registration.cancellation().is_cancelled());
    let published = fixture.api.share_book().publication().shares;
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].resource_key, other.resource_key);
}
