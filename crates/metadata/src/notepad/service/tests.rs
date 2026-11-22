use std::sync::Arc;

use domain::workspace::notepad::{Author, AuthorKind, NotepadError, Write, WriteMode};
use domain::workspace::records::{PersistedWorkspaceKind, PersistedWorkspaceRecord};
use domain::workspace::registry::{WorkspaceArchiveContext, WorkspaceMutationContext};
use model::workspace::notepad::NotepadStore;
use model::workspace::registry::WorkspaceRegistry;
use persistence::storage::notepad::SqliteNotepadStore;
use persistence::storage::registry::FileBackedWorkspaceRegistry;

use super::{NotepadFailure, WorkspaceNotepads};

pub(crate) const NOW: &str = "2026-11-21T19:00:00.000Z";

/// Notepads over real file adapters in `root`, with the active Workspaces `wks_a` and `wks_b`.
pub(crate) struct Fixture {
    pub(crate) notepads: WorkspaceNotepads,
    pub(crate) workspaces: FileBackedWorkspaceRegistry,
    pub(crate) store: Arc<SqliteNotepadStore>,
    _root: tempfile::TempDir,
}

pub(crate) fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let workspaces = FileBackedWorkspaceRegistry::new(root.path().join("workspaces.json"));
    for id in ["wks_a", "wks_b"] {
        workspaces
            .upsert(&workspace(id), WorkspaceMutationContext::default())
            .unwrap();
    }
    let store = Arc::new(SqliteNotepadStore::open(&root.path().join("notepads.sqlite3")).unwrap());
    Fixture {
        notepads: WorkspaceNotepads::new(store.clone(), Box::new(workspaces.clone())),
        workspaces,
        store,
        _root: root,
    }
}

fn workspace(id: &str) -> PersistedWorkspaceRecord {
    PersistedWorkspaceRecord {
        workspace_id: id.to_owned(),
        project_id: "prj_one".to_owned(),
        cwd: format!("/repo/{id}"),
        kind: PersistedWorkspaceKind::Directory,
        display_name: id.to_owned(),
        title: None,
        branch: None,
        worktree_root: None,
        base_branch: None,
        is_paseo_owned_worktree: false,
        main_repo_root: None,
        created_at: NOW.to_owned(),
        updated_at: NOW.to_owned(),
        archived_at: None,
        auto_archived_change_request_url: None,
        pinned_at: None,
        labels: None,
        auto_name: None,
        untrusted_source: None,
        sandbox: None,
    }
}

fn append(text: &str) -> Write<'_> {
    Write {
        text,
        mode: WriteMode::Append,
        expected: None,
        author: Author::anonymous(AuthorKind::User),
        now: NOW,
    }
}

#[test]
fn every_active_workspace_has_its_own_notepad_from_the_start() {
    let fixture = fixture();
    let empty = fixture.notepads.notepad("wks_a").unwrap();
    assert_eq!((empty.text(), empty.revision()), ("", 0));

    let written = fixture.notepads.write("wks_a", append("first")).unwrap();
    assert_eq!(written.revision(), 1);
    fixture.notepads.write("wks_a", append("second")).unwrap();
    assert_eq!(fixture.store.load("wks_a").unwrap().revision(), 2);
    assert_eq!(fixture.notepads.notepad("wks_b").unwrap().revision(), 0);
}

#[test]
fn a_refused_write_stores_nothing_and_returns_the_current_notepad() {
    let fixture = fixture();
    let current = fixture.notepads.write("wks_a", append("text")).unwrap();
    let mut stale = append("replacement");
    stale.mode = WriteMode::Replace;
    assert_eq!(
        fixture.notepads.write("wks_a", stale),
        Err(NotepadFailure::Refused {
            reason: NotepadError::Conflict,
            current: Box::new(current.clone()),
        })
    );
    assert_eq!(fixture.store.load("wks_a").unwrap(), current);
}

#[test]
fn a_notepad_is_kept_while_its_workspace_is_archived_and_dropped_when_it_is_removed() {
    let fixture = fixture();
    assert_eq!(
        fixture.notepads.notepad("wks_missing"),
        Err(NotepadFailure::Workspace)
    );
    fixture.notepads.write("wks_a", append("text")).unwrap();
    fixture.notepads.write("wks_b", append("text")).unwrap();

    fixture
        .workspaces
        .archive("wks_a", NOW, &WorkspaceArchiveContext::default())
        .unwrap();
    assert_eq!(
        fixture.notepads.notepad("wks_a"),
        Err(NotepadFailure::Workspace)
    );
    assert_eq!(
        fixture.notepads.write("wks_a", append("text")),
        Err(NotepadFailure::Workspace)
    );
    assert_eq!(fixture.store.load("wks_a").unwrap().revision(), 1);

    fixture.workspaces.remove("wks_b").unwrap();
    assert_eq!(fixture.store.load("wks_b").unwrap().revision(), 0);
}
