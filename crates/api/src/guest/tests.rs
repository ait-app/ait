use std::time::Instant;

use model::share::Visitor;
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
    assert_eq!(session.workspace_id(), fixture.shared);
    assert_eq!(session.root, fixture.shared_path());
    assert!(!session.cancellation().is_cancelled());
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
            workspace_key: fixture.other.clone(),
            ..visitor.clone()
        },
        Visitor {
            organization_id: Uuid::new_v4(),
            ..visitor.clone()
        },
        Visitor {
            permission: "admin".into(),
            ..visitor.clone()
        },
        Visitor {
            workspace_key: "missing".into(),
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
        "workspaceKey": fixture.shared,
        "organizationId": fixture.organization,
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

#[tokio::test]
async fn paths_must_resolve_inside_the_workspace() {
    let fixture = Workspaces::new(address());
    let session = admitted(&fixture).await;
    let root = fixture.shared_path();
    std::fs::create_dir(root.join("nested")).unwrap();
    let inside = root.join("nested");
    assert!(session.contains(inside.to_str().unwrap()).await);
    assert!(session.contains(root.to_str().unwrap()).await);
    let escape = root.join("..").join("other");
    assert!(!session.contains(escape.to_str().unwrap()).await);
    let other = fixture.root.path().join("other");
    assert!(!session.contains(other.to_str().unwrap()).await);
    assert!(!session.contains("nested").await);
    assert!(
        !session
            .contains(root.join("missing").to_str().unwrap())
            .await
    );
    #[cfg(unix)]
    {
        let link = root.join("link");
        std::os::unix::fs::symlink(&other, &link).unwrap();
        assert!(!session.contains(link.to_str().unwrap()).await);
    }
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
        .remove(&fixture.shared, fixture.organization)
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
