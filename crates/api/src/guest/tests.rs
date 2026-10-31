use std::time::Instant;

use model::share::{ResourceKind, Visitor};
use serde_json::json;
use uuid::Uuid;

use super::*;
use crate::test_support::{Workspaces, address};

async fn admitted(fixture: &Workspaces) -> Session {
    let visitor = fixture.accept_share().await;
    admit(&fixture.api.shared, &visitor.encode()).await.unwrap()
}

#[tokio::test]
async fn accepted_shares_admit_their_visitors() {
    let fixture = Workspaces::new(address());
    let session = admitted(&fixture).await;
    let Reach::Workspace { id, root, .. } = &session.reach else {
        panic!("workspace share resolved to {:?}", session.reach);
    };
    assert_eq!((id, root), (&fixture.shared, &fixture.shared_path()));
    assert_eq!(session.kind(), ResourceKind::Workspace);
    assert!(!session.cancellation().is_cancelled());
}

#[tokio::test]
async fn project_and_node_shares_admit_their_visitors() {
    let fixture = Workspaces::new(address());
    let state = &fixture.api.shared;
    let project = fixture
        .accept(ResourceKind::Project, &fixture.project)
        .await;
    let session = admit(state, &project.encode()).await.unwrap();
    assert_eq!(session.project_id(), Some(fixture.project.as_str()));
    let Reach::Project { root, .. } = &session.reach else {
        panic!("project share resolved to {:?}", session.reach);
    };
    assert_eq!(root, &fixture.shared_path());
    for refused in [
        Visitor {
            resource_key: fixture.other_project.clone(),
            ..project.clone()
        },
        Visitor {
            resource_key: "prj_missing".into(),
            ..project.clone()
        },
        Visitor {
            resource_kind: ResourceKind::Workspace,
            resource_key: fixture.shared.clone(),
            ..project.clone()
        },
    ] {
        assert_eq!(
            admit(state, &refused.encode()).await.err(),
            Some(AdmissionError::Forbidden)
        );
    }

    let node = fixture.accept(ResourceKind::Node, "").await;
    let session = admit(state, &node.encode()).await.unwrap();
    assert_eq!(session.kind(), ResourceKind::Node);
    assert_eq!(session.project_id(), None);
    // A node share is keyed by the empty string; any other key is refused.
    let keyed = Visitor {
        resource_key: fixture.shared.clone(),
        ..node
    };
    assert_eq!(
        admit(state, &keyed.encode()).await.err(),
        Some(AdmissionError::Forbidden)
    );
}

#[tokio::test]
async fn removed_projects_refuse_their_visitors() {
    let fixture = Workspaces::new(address());
    let project = fixture
        .accept(ResourceKind::Project, &fixture.project)
        .await;
    let directory = fixture.api.shared.metadata.directory.as_ref().unwrap();
    let removed = metadata::directory::rpc::directory::execute(
        &mut directory.lock().unwrap(),
        "project.remove.request",
        json!({"projectId": fixture.project}),
    )
    .unwrap();
    assert_eq!(removed["accepted"], true);
    assert_eq!(
        admit(&fixture.api.shared, &project.encode()).await.err(),
        Some(AdmissionError::Forbidden)
    );
}

#[tokio::test]
async fn malformed_and_unadmitted_visitors_are_refused() {
    let fixture = Workspaces::new(address());
    let state = &fixture.api.shared;
    assert_eq!(
        admit(state, "not base64!").await.err(),
        Some(AdmissionError::Malformed)
    );
    let visitor = fixture.accept_share().await;
    for refused in [
        Visitor {
            share_id: Uuid::new_v4(),
            ..visitor.clone()
        },
        Visitor {
            resource_key: fixture.other.clone(),
            ..visitor.clone()
        },
        Visitor {
            group_id: Uuid::new_v4(),
            ..visitor.clone()
        },
        Visitor {
            permission: "admin".into(),
            ..visitor.clone()
        },
        Visitor {
            resource_key: "missing".into(),
            ..visitor.clone()
        },
    ] {
        assert_eq!(
            admit(state, &refused.encode()).await.err(),
            Some(AdmissionError::Forbidden)
        );
    }
    fixture.archive(&fixture.shared);
    assert_eq!(
        admit(state, &visitor.encode()).await.err(),
        Some(AdmissionError::Forbidden)
    );
    assert_eq!(
        StatusCode::from(AdmissionError::Malformed),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        StatusCode::from(AdmissionError::Unavailable),
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn members_audiences_admit_only_recipients() {
    let fixture = Workspaces::new(address());
    let visitor = fixture.accept_share().await;
    let recipient = Uuid::new_v4();
    let request = serde_json::from_value(json!({
        "resourceKind": "workspace",
        "resourceKey": fixture.shared,
        "groupId": fixture.group,
        "permission": "operate",
        "audience": "members",
        "recipients": [recipient],
    }))
    .unwrap();
    fixture.api.shared.shares.set(request).await.unwrap();
    let state = &fixture.api.shared;
    assert_eq!(
        admit(state, &visitor.encode()).await.err(),
        Some(AdmissionError::Forbidden)
    );
    let invited = Visitor {
        user_id: recipient,
        ..visitor
    };
    assert!(admit(state, &invited.encode()).await.is_ok());
}

async fn contains(session: &Session, fixture: &Workspaces, path: &Path) -> bool {
    session
        .contains(&fixture.api.shared, path.to_str().unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn paths_must_resolve_inside_the_workspace() {
    let fixture = Workspaces::new(address());
    let session = admitted(&fixture).await;
    let root = fixture.shared_path();
    std::fs::create_dir(root.join("nested")).unwrap();
    assert!(contains(&session, &fixture, &root.join("nested")).await);
    assert!(contains(&session, &fixture, &root).await);
    let escape = root.join("..").join("other");
    assert!(!contains(&session, &fixture, &escape).await);
    let other = fixture.root.path().join("other");
    assert!(!contains(&session, &fixture, &other).await);
    assert!(!contains(&session, &fixture, &fixture.path("sibling")).await);
    assert!(
        !session
            .contains(&fixture.api.shared, "nested")
            .await
            .unwrap()
    );
    assert!(!contains(&session, &fixture, &root.join("missing")).await);
    #[cfg(unix)]
    {
        let link = root.join("link");
        std::os::unix::fs::symlink(&other, &link).unwrap();
        assert!(!contains(&session, &fixture, &link).await);
    }
}

#[tokio::test]
async fn project_paths_cover_the_root_and_every_active_workspace() {
    let fixture = Workspaces::new(address());
    let project = fixture
        .accept(ResourceKind::Project, &fixture.project)
        .await;
    let session = admit(&fixture.api.shared, &project.encode()).await.unwrap();
    let sibling = fixture.path("sibling");
    std::fs::create_dir(sibling.join("src")).unwrap();
    assert!(contains(&session, &fixture, &fixture.shared_path()).await);
    assert!(contains(&session, &fixture, &sibling.join("src")).await);
    assert!(!contains(&session, &fixture, &fixture.path("other")).await);
    assert!(!contains(&session, &fixture, &fixture.path("registry")).await);
    let state = &fixture.api.shared;
    assert!(session.reaches(state, &fixture.sibling).await.unwrap());
    assert!(!session.reaches(state, &fixture.other).await.unwrap());
    assert!(
        session
            .opens(state, sibling.to_str().unwrap())
            .await
            .unwrap()
    );
    assert!(
        !session
            .opens(state, sibling.join("src").to_str().unwrap())
            .await
            .unwrap()
    );
    let mut ids = session.project_workspace_ids(state).await.unwrap();
    ids.sort();
    let mut expected = vec![fixture.shared.clone(), fixture.sibling.clone()];
    expected.sort();
    assert_eq!(ids, expected);

    // An archived workspace keeps its project but its directory leaves the scope.
    fixture.archive(&fixture.sibling);
    session.reload(state).await.unwrap();
    assert!(!contains(&session, &fixture, &sibling).await);
    assert!(session.reaches(state, &fixture.sibling).await.unwrap());

    let node = fixture.accept(ResourceKind::Node, "").await;
    let session = admit(state, &node.encode()).await.unwrap();
    assert!(contains(&session, &fixture, &fixture.path("other")).await);
    assert!(session.contains(state, "relative").await.unwrap());
    assert!(session.reaches(state, &fixture.other).await.unwrap());
    assert!(session.opens(state, "/").await.unwrap());
    assert_eq!(session.directory(state, &fixture.other).await, Ok(None));
}

#[tokio::test]
async fn download_tokens_are_bounded_per_connection() {
    let fixture = Workspaces::new(address());
    let session = admitted(&fixture).await;
    let start = Instant::now();
    for _ in 0..MAX_DOWNLOADS {
        assert!(session.take_download(start));
    }
    assert!(!session.take_download(start));
    assert!(session.take_download(start + DOWNLOAD_TTL));
}

#[tokio::test]
async fn revocation_cancels_the_session() {
    let fixture = Workspaces::new(address());
    let session = admitted(&fixture).await;
    fixture
        .api
        .shared
        .shares
        .remove(ResourceKind::Workspace, &fixture.shared, fixture.group)
        .await
        .unwrap();
    assert!(session.cancellation().is_cancelled());
}

#[test]
fn lookups_never_disclose_other_resources() {
    assert_eq!(conceal(ErrorCode::AgentNotFound), ErrorCode::Forbidden);
    assert_eq!(conceal(ErrorCode::InvalidMessage), ErrorCode::Forbidden);
    assert_eq!(
        conceal(ErrorCode::ResourceExhausted),
        ErrorCode::ResourceExhausted
    );
}
