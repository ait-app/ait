use std::path::{Path, PathBuf};
use std::sync::Arc;

use domain::workspace::notepad::{Author, AuthorKind, NotepadError, Write, WriteMode};
use domain::workspace::records::{PersistedWorkspaceKind, PersistedWorkspaceRecord};
use domain::workspace::registry::{WorkspaceArchiveContext, WorkspaceMutationContext};
use model::workspace::notepad::{NotepadStore, NotepadStoreError};
use model::workspace::registry::WorkspaceRegistry;
use persistence::storage::notepad::FileNotepadStore;
use persistence::storage::registry::FileBackedWorkspaceRegistry;

use super::{NotepadFailure, WorkspaceNotepads};

pub(crate) const NOW: &str = "2026-11-21T19:00:00.000Z";

/// Notepads over real file adapters in `root`, with the active Workspaces `wks_a` and `wks_b`
/// whose checkouts are `root/<id>`.
pub(crate) struct Fixture {
    pub(crate) notepads: WorkspaceNotepads,
    pub(crate) workspaces: FileBackedWorkspaceRegistry,
    pub(crate) root: tempfile::TempDir,
}

impl Fixture {
    /// Where the notepad file of `workspace` is kept.
    pub(crate) fn file(&self, workspace: &str) -> PathBuf {
        self.root
            .path()
            .join("notepads")
            .join(workspace)
            .join("notepad.md")
    }
}

pub(crate) fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let workspaces = FileBackedWorkspaceRegistry::new(root.path().join("workspaces.json"));
    for id in ["wks_a", "wks_b"] {
        let checkout = root.path().join(id);
        std::fs::create_dir(&checkout).unwrap();
        workspaces
            .upsert(
                &workspace(id, &checkout),
                WorkspaceMutationContext::default(),
            )
            .unwrap();
    }
    let store = Arc::new(FileNotepadStore::new(root.path().join("notepads")));
    Fixture {
        notepads: WorkspaceNotepads::new(store, Box::new(workspaces.clone())),
        workspaces,
        root,
    }
}

fn workspace(id: &str, checkout: &Path) -> PersistedWorkspaceRecord {
    PersistedWorkspaceRecord {
        workspace_id: id.to_owned(),
        project_id: "prj_one".to_owned(),
        cwd: checkout.to_string_lossy().into_owned(),
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

fn replace<'a>(text: &'a str, expected: Option<&'a str>) -> Write<'a> {
    Write {
        mode: WriteMode::Replace,
        expected,
        ..append(text)
    }
}

#[test]
fn every_active_workspace_has_its_own_notepad_file_outside_its_checkout() {
    let fixture = fixture();
    let empty = fixture.notepads.notepad("wks_a").unwrap();
    assert_eq!(empty.text(), "");
    assert_eq!(Path::new(empty.path()), fixture.file("wks_a"));
    assert!(fixture.file("wks_a").is_file());
    assert!(!fixture.file("wks_b").exists());

    let written = fixture.notepads.write("wks_a", &append("first")).unwrap();
    assert_ne!(written.revision(), empty.revision());
    fixture.notepads.write("wks_a", &append("second")).unwrap();
    assert_eq!(
        std::fs::read_to_string(fixture.file("wks_a")).unwrap(),
        format!("[User · {NOW}]\nfirst\n\n[User · {NOW}]\nsecond\n")
    );
    assert_eq!(fixture.notepads.notepad("wks_b").unwrap().text(), "");
    assert_eq!(
        std::fs::read_dir(fixture.root.path().join("wks_a"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn a_write_sees_what_the_file_editor_saved() {
    let fixture = fixture();
    let read = fixture.notepads.notepad("wks_a").unwrap();
    std::fs::write(fixture.file("wks_a"), "typed in the editor\n").unwrap();
    assert_eq!(
        fixture
            .notepads
            .write("wks_a", &append("from an agent"))
            .unwrap()
            .text(),
        format!("typed in the editor\n\n[User · {NOW}]\nfrom an agent\n")
    );
    assert!(matches!(
        fixture
            .notepads
            .write("wks_a", &replace("stale", Some(read.revision()))),
        Err(NotepadFailure::Refused {
            reason: NotepadError::Conflict,
            ..
        })
    ));
}

#[test]
fn a_refused_write_stores_nothing_and_returns_the_current_notepad() {
    let fixture = fixture();
    let current = fixture.notepads.write("wks_a", &append("text")).unwrap();
    assert_eq!(
        fixture
            .notepads
            .write("wks_a", &replace("replacement", None)),
        Err(NotepadFailure::Refused {
            reason: NotepadError::Conflict,
            current: Box::new(current.clone()),
        })
    );
    assert_eq!(fixture.notepads.notepad("wks_a").unwrap(), current);
    let replaced = fixture
        .notepads
        .write("wks_a", &replace("replacement", Some(current.revision())))
        .unwrap();
    assert_eq!(replaced.text(), "replacement");
}

#[test]
fn a_new_notepad_starts_with_the_notebook_file_of_the_checkout() {
    let fixture = fixture();
    let legacy = fixture.root.path().join("wks_a/.ait-notebook.md");
    std::fs::write(&legacy, "old notes").unwrap();
    assert_eq!(
        fixture.notepads.notepad("wks_a").unwrap().text(),
        "old notes"
    );
    assert_eq!(std::fs::read_to_string(&legacy).unwrap(), "old notes");
    assert_eq!(fixture.notepads.notepad("wks_b").unwrap().text(), "");
}

#[test]
fn a_notepad_is_kept_while_its_workspace_is_archived_and_dropped_when_it_is_removed() {
    let fixture = fixture();
    assert_eq!(
        fixture.notepads.notepad("wks_missing"),
        Err(NotepadFailure::Workspace)
    );
    fixture.notepads.write("wks_a", &append("text")).unwrap();
    fixture.notepads.write("wks_b", &append("text")).unwrap();

    fixture
        .workspaces
        .archive("wks_a", NOW, &WorkspaceArchiveContext::default())
        .unwrap();
    assert_eq!(
        fixture.notepads.notepad("wks_a"),
        Err(NotepadFailure::Workspace)
    );
    assert_eq!(
        fixture.notepads.write("wks_a", &append("text")),
        Err(NotepadFailure::Workspace)
    );
    assert!(fixture.file("wks_a").is_file());

    fixture.workspaces.remove("wks_b").unwrap();
    assert!(!fixture.file("wks_b").parent().unwrap().exists());
}

#[test]
fn a_notepad_directory_names_its_workspace() {
    let fixture = fixture();
    let directory = fixture.file("wks_a").parent().unwrap().to_owned();
    assert_eq!(
        fixture.notepads.workspace_of(&directory).as_deref(),
        Some("wks_a")
    );
    assert_eq!(
        fixture
            .notepads
            .workspace_of(&fixture.root.path().join("wks_a")),
        None
    );
}

/// A store whose file changes under every save.
#[derive(Debug)]
struct Contended(FileNotepadStore);

impl NotepadStore for Contended {
    fn open(
        &self,
        workspace_id: &str,
        legacy: Option<&Path>,
    ) -> Result<domain::workspace::notepad::Notepad, NotepadStoreError> {
        self.0.open(workspace_id, legacy)
    }

    fn save(
        &self,
        _: &str,
        _: &domain::workspace::notepad::Notepad,
        _: &str,
    ) -> Result<domain::workspace::notepad::Notepad, NotepadStoreError> {
        Err(NotepadStoreError::Changed)
    }

    fn clear(&self, workspace_id: &str) -> Result<(), NotepadStoreError> {
        self.0.clear(workspace_id)
    }

    fn owner(&self, directory: &Path) -> Option<String> {
        self.0.owner(directory)
    }
}

#[test]
fn a_file_that_keeps_changing_fails_the_write_instead_of_retrying_forever() {
    let fixture = fixture();
    let store = Contended(FileNotepadStore::new(fixture.root.path().join("notepads")));
    let notepads = WorkspaceNotepads::new(Arc::new(store), Box::new(fixture.workspaces.clone()));
    assert_eq!(
        notepads.write("wks_a", &append("text")),
        Err(NotepadFailure::Storage(NotepadStoreError::Changed))
    );
}
