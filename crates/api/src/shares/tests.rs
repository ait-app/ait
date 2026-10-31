use std::sync::atomic::{AtomicBool, Ordering};

use model::share::{Accepted, Audience, Rejected, ResourceKind, VisitorGrant};
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

fn set(workspace: &str, group: Uuid, recipients: &[Uuid]) -> SetRequest {
    let audience = if recipients.is_empty() {
        "group"
    } else {
        "members"
    };
    serde_json::from_value(json!({
        "resourceKind": "workspace",
        "resourceKey": workspace,
        "groupId": group,
        "permission": "operate",
        "audience": audience,
        "recipients": recipients,
    }))
    .unwrap()
}

/// The setting ID of the workspace share `workspace` in `group`.
fn setting(shares: &Shares, workspace: &str, group: Uuid) -> Uuid {
    let book = lock(&shares.book);
    let index = book
        .position(ResourceKind::Workspace, workspace, group)
        .unwrap();
    book.settings[index].id
}

fn accepted(shares: &Shares, workspace: &str, group: Uuid, share_id: Uuid) -> Outcome {
    Outcome {
        revision: 1,
        accepted: vec![Accepted {
            share_id,
            resource_kind: ResourceKind::Workspace,
            resource_key: workspace.into(),
            group_id: group,
            setting_id: setting(shares, workspace, group),
            missing_recipients: Vec::new(),
        }],
        rejected: Vec::new(),
        denied: Vec::new(),
        groups: vec![Group {
            group_id: group,
            name: "Team".into(),
            status: "active".into(),
            my_role: "admin".into(),
        }],
    }
}

fn rejected(shares: &Shares, workspace: &str, group: Uuid, code: &str) -> Outcome {
    Outcome {
        revision: 1,
        rejected: vec![Rejected {
            resource_kind: ResourceKind::Workspace,
            resource_key: workspace.into(),
            group_id: group,
            setting_id: setting(shares, workspace, group),
            code: code.into(),
        }],
        ..Outcome::default()
    }
}

fn visitor(share_id: Uuid, group: Uuid, user_id: Uuid) -> Visitor {
    Visitor {
        user_id,
        display_name: None,
        group_id: group,
        share_id,
        resource_kind: ResourceKind::Workspace,
        resource_key: "w".into(),
        permission: "operate".into(),
    }
}

#[tokio::test]
async fn set_bumps_revision_only_for_published_changes() {
    let shares = shares(None);
    let mut changes = shares.changes();
    let group = Uuid::new_v4();
    let stored = shares.set(set("w", group, &[])).await.unwrap();
    assert_eq!(stored["share"]["state"], "pending");
    let setting_id = stored["share"]["settingId"].clone();
    assert!(setting_id.is_string());
    assert!(stored["share"]["shareId"].is_null());
    assert!(changes.has_changed().unwrap());
    assert_eq!(*changes.borrow_and_update(), 1);
    // Editing keeps the setting ID.
    let edited = shares.set(set("w", group, &[])).await.unwrap();
    assert_eq!(edited["share"]["settingId"], setting_id);
    assert!(!changes.has_changed().unwrap());
    let publication = shares.publication();
    assert_eq!(publication.revision, 1);
    assert_eq!(publication.shares.len(), 1);
    assert_eq!(publication.shares[0].audience, Audience::Group);
    assert_eq!(json!(publication.shares[0].setting_id), setting_id);
}

#[tokio::test]
async fn capacity_is_bounded() {
    let shares = shares(None);
    let group = Uuid::new_v4();
    for index in 0..MAX_SHARES {
        shares
            .set(set(&format!("w{index}"), group, &[]))
            .await
            .unwrap();
    }
    assert_eq!(
        shares.set(set("extra", group, &[])).await,
        Err(ErrorCode::ResourceExhausted)
    );
    shares
        .set(set("w0", group, &[Uuid::new_v4()]))
        .await
        .unwrap();
}

#[test]
fn acceptance_is_persisted_and_republished_with_its_setting() {
    let store = Arc::new(MemoryStore::default());
    let shares = shares(Some(store.clone()));
    let group = Uuid::new_v4();
    let share_id = Uuid::new_v4();
    let ((), commit) = shares.mutate(|book| {
        let stored = upsert(book, set("w", group, &[]));
        ((), stored.is_ok())
    });
    shares.finish_detached(commit);
    shares.apply(accepted(&shares, "w", group, share_id));
    let publication = shares.publication();
    assert_eq!(publication.revision, 1);
    let listed = shares.list();
    assert_eq!(listed["shares"][0]["state"], "accepted");
    assert_eq!(listed["shares"][0]["shareId"], json!(share_id));
    assert_eq!(listed["groups"][0]["name"], "Team");
    assert_eq!(shares.groups()["groups"][0]["myRole"], "admin");

    let reopened = self::shares(Some(store.clone()));
    assert_eq!(reopened.publication(), publication);
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
    *lock(&store.document) = json!({"revision": 7, "shares": [], "groups": []});
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
    assert_eq!(api.share_book().publication().revision, 7);
    assert!(matches!(
        api.with_share_store(store),
        Err(crate::ConfigError::ShareStore)
    ));
}

#[tokio::test]
async fn failed_writes_are_reported_and_retried_by_later_changes() {
    let store = Arc::new(MemoryStore::default());
    let shares = shares(Some(store.clone()));
    let group = Uuid::new_v4();
    store.fail.store(true, Ordering::SeqCst);
    assert_eq!(
        shares.set(set("w", group, &[])).await,
        Err(ErrorCode::RegistryIo)
    );
    store.fail.store(false, Ordering::SeqCst);
    shares.set(set("x", group, &[])).await.unwrap();
    assert_eq!(lock(&store.document)["shares"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn revoked_shares_are_removed_and_republished() {
    let shares = shares(None);
    let group = Uuid::new_v4();
    shares.set(set("w", group, &[])).await.unwrap();
    shares.set(set("x", group, &[])).await.unwrap();
    let mut changes = shares.changes();
    changes.borrow_and_update();
    shares.apply(rejected(&shares, "w", group, REVOKED));
    assert!(changes.has_changed().unwrap());
    let publication = shares.publication();
    assert_eq!(publication.revision, 3);
    assert_eq!(publication.shares.len(), 1);
    assert_eq!(publication.shares[0].resource_key, "x");
}

#[tokio::test]
async fn unlisted_entries_keep_their_state() {
    let shares = shares(None);
    let group = Uuid::new_v4();
    shares.set(set("w", group, &[])).await.unwrap();
    shares.apply(accepted(&shares, "w", group, Uuid::new_v4()));
    shares.apply(Outcome {
        revision: 1,
        ..Outcome::default()
    });
    assert_eq!(shares.list()["shares"][0]["state"], "accepted");
    assert_eq!(shares.list()["groups"], json!([]));
}

#[tokio::test]
async fn guests_end_when_their_share_is_no_longer_accepted() {
    let shares = shares(None);
    let group = Uuid::new_v4();
    let share_id = Uuid::new_v4();
    shares.set(set("w", group, &[])).await.unwrap();
    let guest = visitor(share_id, group, Uuid::new_v4());
    assert!(shares.admit(&guest).is_none());
    shares.apply(accepted(&shares, "w", group, share_id));
    let registration = shares.admit(&guest).unwrap();
    assert!(!registration.cancellation().is_cancelled());
    shares.apply(rejected(&shares, "w", group, "group_inactive"));
    assert!(registration.cancellation().is_cancelled());
    assert!(shares.admit(&guest).is_none());
    assert_eq!(shares.list()["shares"][0]["code"], "group_inactive");
    drop(registration);
    assert_eq!(lock(&shares.book).guests.live(), 0);
}

#[tokio::test]
async fn removing_a_recipient_ends_only_that_visitor() {
    let shares = shares(None);
    let group = Uuid::new_v4();
    let share_id = Uuid::new_v4();
    let (kept, removed) = (Uuid::new_v4(), Uuid::new_v4());
    shares.set(set("w", group, &[kept, removed])).await.unwrap();
    shares.apply(accepted(&shares, "w", group, share_id));
    let keep = shares.admit(&visitor(share_id, group, kept)).unwrap();
    let lose = shares.admit(&visitor(share_id, group, removed)).unwrap();
    assert!(
        shares
            .admit(&visitor(share_id, group, Uuid::new_v4()))
            .is_none()
    );
    shares.set(set("w", group, &[kept])).await.unwrap();
    assert!(lose.cancellation().is_cancelled());
    assert!(!keep.cancellation().is_cancelled());
}

#[tokio::test]
async fn removal_ends_guests_immediately() {
    let shares = shares(None);
    let group = Uuid::new_v4();
    let share_id = Uuid::new_v4();
    shares.set(set("w", group, &[])).await.unwrap();
    shares.apply(accepted(&shares, "w", group, share_id));
    let registration = shares
        .admit(&visitor(share_id, group, Uuid::new_v4()))
        .unwrap();
    assert!(registration.record_terminal("terminal".into()));
    let removed = shares
        .remove(ResourceKind::Workspace, "w", group)
        .await
        .unwrap();
    assert_eq!(removed["removed"], true);
    assert!(registration.cancellation().is_cancelled());
    let publication = shares.publication();
    assert_eq!((publication.revision, publication.shares), (2, Vec::new()));
    assert!(!registration.record_terminal("late".into()));
    let missing = shares
        .remove(ResourceKind::Workspace, "w", group)
        .await
        .unwrap();
    assert_eq!(missing["removed"], false);
    assert_eq!(shares.publication().revision, 2);
}

#[tokio::test]
async fn retired_workspaces_are_forgotten() {
    let shares = shares(None);
    let group = Uuid::new_v4();
    let share_id = Uuid::new_v4();
    shares.set(set("w", group, &[])).await.unwrap();
    shares.set(set("x", group, &[])).await.unwrap();
    shares.apply(accepted(&shares, "w", group, share_id));
    let registration = shares
        .admit(&visitor(share_id, group, Uuid::new_v4()))
        .unwrap();
    shares.forget(ResourceKind::Workspace, &["w".to_owned()]);
    assert!(registration.cancellation().is_cancelled());
    let publication = shares.publication();
    assert_eq!(publication.revision, 3);
    assert_eq!(publication.shares.len(), 1);
    shares.forget(ResourceKind::Project, &["x".to_owned()]);
    shares.forget(ResourceKind::Workspace, &["missing".to_owned()]);
    assert_eq!(shares.publication().revision, 3);
}

#[tokio::test]
async fn shares_are_keyed_by_kind_and_projects_are_forgotten_separately() {
    let shares = shares(None);
    let group = Uuid::new_v4();
    let project: SetRequest = serde_json::from_value(json!({
        "resourceKind": "project",
        "resourceKey": "w",
        "groupId": group,
        "permission": "operate",
        "audience": "group",
    }))
    .unwrap();
    shares.set(set("w", group, &[])).await.unwrap();
    shares.set(project).await.unwrap();
    assert_eq!(shares.publication().shares.len(), 2);
    shares.forget(ResourceKind::Project, &["w".to_owned()]);
    let published = shares.publication().shares;
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].resource_kind, ResourceKind::Workspace);
    let removed = shares
        .remove(ResourceKind::Project, "w", group)
        .await
        .unwrap();
    assert_eq!(
        removed,
        json!({"resourceKind":"project","resourceKey":"w","groupId":group,"removed":false})
    );
}

/// Documents written before shares named groups and resource kinds lose only those entries.
#[test]
fn unreadable_stored_entries_are_dropped() {
    let store = Arc::new(MemoryStore::default());
    let group = Uuid::new_v4();
    *lock(&store.document) = json!({
        "revision": 3,
        "shares": [
            {"workspaceKey": "w", "organizationId": group, "permission": "operate",
                "audience": "organization"},
            {"resourceKind": "node", "resourceKey": "", "groupId": group,
                "permission": "operate", "audience": "group", "settingId": Uuid::new_v4()},
            // A setting without an ID could have been revoked; it is not trusted.
            {"resourceKind": "workspace", "resourceKey": "w", "groupId": group,
                "permission": "operate", "audience": "group"},
        ],
        "orgs": [{"organizationId": group, "name": "Old", "status": "active", "myRole": "admin"}],
        "groups": [{"groupId": group, "name": "Team", "status": "active", "myRole": "admin"},
            {"name": "broken"}],
    });
    let shares = shares(Some(store));
    let publication = shares.publication();
    assert_eq!(publication.revision, 3);
    assert_eq!(publication.shares.len(), 1);
    assert_eq!(publication.shares[0].resource_kind, ResourceKind::Node);
    assert_eq!(shares.groups()["groups"][0]["name"], "Team");
    assert_eq!(shares.groups()["groups"].as_array().map(Vec::len), Some(1));
}

#[tokio::test]
async fn late_outcomes_never_touch_a_newer_setting() {
    let shares = shares(None);
    let group = Uuid::new_v4();
    shares.set(set("w", group, &[])).await.unwrap();
    let revoked = rejected(&shares, "w", group, REVOKED);
    let accepted_first = accepted(&shares, "w", group, Uuid::new_v4());
    shares
        .remove(ResourceKind::Workspace, "w", group)
        .await
        .unwrap();
    // The user shares again: a new setting the old outcomes do not name.
    shares.set(set("w", group, &[])).await.unwrap();
    shares.apply(revoked);
    shares.apply(accepted_first);
    let listed = shares.list();
    assert_eq!(listed["shares"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed["shares"][0]["state"], "pending");

    // An outcome for an older publication is ignored.
    let share_id = Uuid::new_v4();
    let mut newer = accepted(&shares, "w", group, share_id);
    newer.revision = 5;
    shares.apply(newer);
    shares.apply(rejected(&shares, "w", group, "not_member"));
    assert_eq!(shares.list()["shares"][0]["state"], "accepted");
}

#[tokio::test]
async fn denied_visitors_lose_access_while_the_share_stays_accepted() {
    let shares = shares(None);
    let group = Uuid::new_v4();
    let share_id = Uuid::new_v4();
    let (kept, removed) = (Uuid::new_v4(), Uuid::new_v4());
    shares.set(set("w", group, &[])).await.unwrap();
    shares.apply(accepted(&shares, "w", group, share_id));
    let keep = shares.admit(&visitor(share_id, group, kept)).unwrap();
    let lose = shares.admit(&visitor(share_id, group, removed)).unwrap();
    assert!(lose.record_terminal("terminal".into()));
    // Both grants are published so the center can report either.
    let mut visitors = shares.publication().visitors;
    visitors.sort();
    let mut expected = vec![
        VisitorGrant {
            share_id,
            user_id: kept,
        },
        VisitorGrant {
            share_id,
            user_id: removed,
        },
    ];
    expected.sort();
    assert_eq!(visitors, expected);
    drop(lose);

    // The visitor is offline; the grant still ends, with what it started.
    let mut outcome = accepted(&shares, "w", group, share_id);
    outcome.denied = vec![VisitorGrant {
        share_id,
        user_id: removed,
    }];
    shares.apply(outcome);
    assert!(!keep.cancellation().is_cancelled());
    assert!(shares.admit(&visitor(share_id, group, removed)).is_none());
    assert_eq!(
        shares.publication().visitors,
        vec![VisitorGrant {
            share_id,
            user_id: kept,
        }]
    );
    assert_eq!(shares.list()["shares"][0]["state"], "accepted");
}

#[tokio::test]
async fn recipients_that_are_not_members_are_not_admitted() {
    let shares = shares(None);
    let group = Uuid::new_v4();
    let share_id = Uuid::new_v4();
    let (member, former) = (Uuid::new_v4(), Uuid::new_v4());
    shares
        .set(set("w", group, &[member, former]))
        .await
        .unwrap();
    let registration = shares.admit(&visitor(share_id, group, former));
    assert!(registration.is_none());
    let mut outcome = accepted(&shares, "w", group, share_id);
    outcome.accepted[0].missing_recipients = vec![former];
    shares.apply(outcome);
    assert!(shares.admit(&visitor(share_id, group, member)).is_some());
    assert!(shares.admit(&visitor(share_id, group, former)).is_none());
}

#[test]
fn listed_agent_ids_are_extracted() {
    let listed = json!({"entries":[{"agent":{"id":"a"}},{"agent":{}},{"agent":{"id":"b"}}]});
    assert_eq!(cleanup::agent_ids(&listed), vec!["a", "b"]);
    assert!(cleanup::agent_ids(&json!({})).is_empty());
}
