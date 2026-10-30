use std::sync::atomic::{AtomicBool, Ordering};

use model::share::{Accepted, Audience, Rejected};
use serde_json::json;

use super::*;
use crate::test_support::{TOKEN, address};
use crate::{Api, Services};

#[derive(Debug, Default)]
struct MemoryStore {
    document: Mutex<Value>,
    fail: AtomicBool,
}

impl OutboxStore for MemoryStore {
    fn load(&self) -> Result<Value, StoreError> {
        Ok(lock(&self.document).clone())
    }

    fn save(&self, document: &Value) -> Result<(), StoreError> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(StoreError);
        }
        *lock(&self.document) = document.clone();
        Ok(())
    }
}

fn shares(store: Option<Arc<MemoryStore>>) -> Arc<Shares> {
    let api = Api::new(
        address(),
        "server".into(),
        "instance".into(),
        TOKEN.into(),
        Services::default(),
    )
    .unwrap();
    let shares = Shares::new(&api.shared.terminal, &api.shared.provider);
    if let Some(store) = store {
        shares.open(store).unwrap();
    }
    Arc::new(shares)
}

fn set(workspace: &str, organization: Uuid, recipients: &[Uuid]) -> SetRequest {
    let audience = if recipients.is_empty() {
        "organization"
    } else {
        "members"
    };
    serde_json::from_value(json!({
        "workspaceKey": workspace,
        "organizationId": organization,
        "permission": "operate",
        "audience": audience,
        "recipients": recipients,
    }))
    .unwrap()
}

fn accepted(workspace: &str, organization: Uuid, share_id: Uuid) -> Outcome {
    Outcome {
        revision: Some(1),
        accepted: vec![Accepted {
            share_id,
            workspace_key: workspace.into(),
            organization_id: organization,
            generation: 5,
            missing_recipients: Vec::new(),
        }],
        rejected: Vec::new(),
        orgs: vec![Organization {
            organization_id: organization,
            name: "Team".into(),
            status: "active".into(),
            my_role: "admin".into(),
        }],
    }
}

fn rejected(workspace: &str, organization: Uuid, code: &str) -> Outcome {
    Outcome {
        rejected: vec![Rejected {
            workspace_key: workspace.into(),
            organization_id: organization,
            code: code.into(),
        }],
        ..Outcome::default()
    }
}

fn visitor(share_id: Uuid, organization: Uuid, user_id: Uuid) -> Visitor {
    Visitor {
        user_id,
        display_name: None,
        organization_id: organization,
        share_id,
        workspace_key: "w".into(),
        permission: "operate".into(),
    }
}

#[tokio::test]
async fn set_bumps_revision_only_for_published_changes() {
    let shares = shares(None);
    let mut changes = shares.changes();
    let organization = Uuid::new_v4();
    let stored = shares.set(set("w", organization, &[])).await.unwrap();
    assert_eq!(stored["share"]["state"], "pending");
    assert!(stored["share"]["generation"].is_null());
    assert!(stored["share"]["shareId"].is_null());
    assert!(changes.has_changed().unwrap());
    assert_eq!(*changes.borrow_and_update(), 1);
    shares.set(set("w", organization, &[])).await.unwrap();
    assert!(!changes.has_changed().unwrap());
    let (revision, published) = shares.publication();
    assert_eq!(revision, 1);
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].audience, Audience::Organization);
    assert_eq!(published[0].generation, None);
}

#[tokio::test]
async fn capacity_is_bounded() {
    let shares = shares(None);
    let organization = Uuid::new_v4();
    for index in 0..MAX_SHARES {
        shares
            .set(set(&format!("w{index}"), organization, &[]))
            .await
            .unwrap();
    }
    assert_eq!(
        shares.set(set("extra", organization, &[])).await,
        Err(ErrorCode::ResourceExhausted)
    );
    shares
        .set(set("w0", organization, &[Uuid::new_v4()]))
        .await
        .unwrap();
}

#[test]
fn acceptance_is_persisted_and_republished_with_its_generation() {
    let store = Arc::new(MemoryStore::default());
    let shares = shares(Some(store.clone()));
    let organization = Uuid::new_v4();
    let share_id = Uuid::new_v4();
    let ((), commit) = shares.mutate(|book| {
        let stored = upsert(book, set("w", organization, &[]));
        ((), stored.is_ok())
    });
    shares.finish_detached(commit);
    shares.apply(accepted("w", organization, share_id));
    let (revision, published) = shares.publication();
    assert_eq!(revision, 1);
    assert_eq!(published[0].generation, Some(5));
    let listed = shares.list();
    assert_eq!(listed["shares"][0]["state"], "accepted");
    assert_eq!(listed["shares"][0]["shareId"], json!(share_id));
    assert_eq!(listed["orgs"][0]["name"], "Team");
    assert_eq!(shares.orgs()["orgs"][0]["myRole"], "admin");

    let reopened = self::shares(Some(store.clone()));
    assert_eq!(reopened.publication(), (1, published));
    assert_eq!(reopened.list(), listed);
    assert_eq!(*reopened.changes().borrow(), 1);
}

#[test]
fn malformed_store_is_refused() {
    let store = Arc::new(MemoryStore::default());
    *lock(&store.document) = json!({"shares": "invalid"});
    let api = Api::new(
        address(),
        "server".into(),
        "instance".into(),
        TOKEN.into(),
        Services::default(),
    )
    .unwrap();
    let shares = Shares::new(&api.shared.terminal, &api.shared.provider);
    assert_eq!(shares.open(store), Err(StoreError));
}

/// The daemon attaches the store after runtime services already hold the book.
#[test]
fn the_daemon_builder_attaches_a_store_once() {
    let store = Arc::new(MemoryStore::default());
    *lock(&store.document) = json!({"revision": 7, "shares": [], "orgs": []});
    let api = Api::new(
        address(),
        "server".into(),
        "instance".into(),
        TOKEN.into(),
        Services::default(),
    )
    .unwrap()
    .with_browser_origins(Vec::new())
    .unwrap()
    .with_share_store(store.clone())
    .unwrap();
    assert_eq!(api.share_book().publication().0, 7);
    assert!(matches!(
        api.with_share_store(store),
        Err(crate::ConfigError::ShareStore)
    ));
}

#[tokio::test]
async fn failed_writes_are_reported_and_retried_by_later_changes() {
    let store = Arc::new(MemoryStore::default());
    let shares = shares(Some(store.clone()));
    let organization = Uuid::new_v4();
    store.fail.store(true, Ordering::SeqCst);
    assert_eq!(
        shares.set(set("w", organization, &[])).await,
        Err(ErrorCode::RegistryIo)
    );
    store.fail.store(false, Ordering::SeqCst);
    shares.set(set("x", organization, &[])).await.unwrap();
    assert_eq!(lock(&store.document)["shares"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn revoked_shares_are_removed_and_republished() {
    let shares = shares(None);
    let organization = Uuid::new_v4();
    shares.set(set("w", organization, &[])).await.unwrap();
    shares.set(set("x", organization, &[])).await.unwrap();
    let mut changes = shares.changes();
    changes.borrow_and_update();
    shares.apply(rejected("w", organization, REVOKED));
    assert!(changes.has_changed().unwrap());
    let (revision, published) = shares.publication();
    assert_eq!(revision, 3);
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].workspace_key, "x");
}

#[tokio::test]
async fn unlisted_entries_keep_their_state() {
    let shares = shares(None);
    let organization = Uuid::new_v4();
    shares.set(set("w", organization, &[])).await.unwrap();
    shares.apply(accepted("w", organization, Uuid::new_v4()));
    shares.apply(Outcome::default());
    assert_eq!(shares.list()["shares"][0]["state"], "accepted");
    assert_eq!(shares.list()["orgs"], json!([]));
}

#[tokio::test]
async fn guests_end_when_their_share_is_no_longer_accepted() {
    let shares = shares(None);
    let organization = Uuid::new_v4();
    let share_id = Uuid::new_v4();
    shares.set(set("w", organization, &[])).await.unwrap();
    let guest = visitor(share_id, organization, Uuid::new_v4());
    assert!(shares.admit(&guest).is_none());
    shares.apply(accepted("w", organization, share_id));
    let registration = shares.admit(&guest).unwrap();
    assert!(!registration.cancellation().is_cancelled());
    shares.apply(rejected("w", organization, "org_inactive"));
    assert!(registration.cancellation().is_cancelled());
    assert!(shares.admit(&guest).is_none());
    assert_eq!(shares.list()["shares"][0]["code"], "org_inactive");
    drop(registration);
    assert_eq!(lock(&shares.book).guests.live(), 0);
}

#[tokio::test]
async fn removing_a_recipient_ends_only_that_visitor() {
    let shares = shares(None);
    let organization = Uuid::new_v4();
    let share_id = Uuid::new_v4();
    let (kept, removed) = (Uuid::new_v4(), Uuid::new_v4());
    shares
        .set(set("w", organization, &[kept, removed]))
        .await
        .unwrap();
    shares.apply(accepted("w", organization, share_id));
    let keep = shares
        .admit(&visitor(share_id, organization, kept))
        .unwrap();
    let lose = shares
        .admit(&visitor(share_id, organization, removed))
        .unwrap();
    assert!(
        shares
            .admit(&visitor(share_id, organization, Uuid::new_v4()))
            .is_none()
    );
    shares.set(set("w", organization, &[kept])).await.unwrap();
    assert!(lose.cancellation().is_cancelled());
    assert!(!keep.cancellation().is_cancelled());
}

#[tokio::test]
async fn removal_ends_guests_immediately() {
    let shares = shares(None);
    let organization = Uuid::new_v4();
    let share_id = Uuid::new_v4();
    shares.set(set("w", organization, &[])).await.unwrap();
    shares.apply(accepted("w", organization, share_id));
    let registration = shares
        .admit(&visitor(share_id, organization, Uuid::new_v4()))
        .unwrap();
    assert!(registration.record_terminal("terminal".into()));
    let removed = shares.remove("w", organization).await.unwrap();
    assert_eq!(removed["removed"], true);
    assert!(registration.cancellation().is_cancelled());
    assert_eq!(shares.publication(), (2, Vec::new()));
    assert!(!registration.record_terminal("late".into()));
    let missing = shares.remove("w", organization).await.unwrap();
    assert_eq!(missing["removed"], false);
    assert_eq!(shares.publication().0, 2);
}

#[tokio::test]
async fn retired_workspaces_are_forgotten() {
    let shares = shares(None);
    let organization = Uuid::new_v4();
    let share_id = Uuid::new_v4();
    shares.set(set("w", organization, &[])).await.unwrap();
    shares.set(set("x", organization, &[])).await.unwrap();
    shares.apply(accepted("w", organization, share_id));
    let registration = shares
        .admit(&visitor(share_id, organization, Uuid::new_v4()))
        .unwrap();
    shares.forget_workspaces(&["w".to_owned()]);
    assert!(registration.cancellation().is_cancelled());
    let (revision, published) = shares.publication();
    assert_eq!(revision, 3);
    assert_eq!(published.len(), 1);
    shares.forget_workspaces(&["missing".to_owned()]);
    assert_eq!(shares.publication().0, 3);
}

#[test]
fn listed_agent_ids_are_extracted() {
    let listed = json!({"entries":[{"agent":{"id":"a"}},{"agent":{}},{"agent":{"id":"b"}}]});
    assert_eq!(cleanup::agent_ids(&listed), vec!["a", "b"]);
    assert!(cleanup::agent_ids(&json!({})).is_empty());
}
