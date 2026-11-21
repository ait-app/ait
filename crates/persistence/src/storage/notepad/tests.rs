use domain::workspace::notepad::{Author, AuthorKind, NoteChange, Notepad, Write, WriteMode};
use model::workspace::notepad::{NotepadStore, NotepadStoreError};

use super::SqliteNotepadStore;

fn write(store: &SqliteNotepadStore, workspace: &str, key: &str, text: &str) {
    let mut notepad = store.load(workspace).unwrap();
    let change = notepad
        .write(Write {
            key,
            text,
            mode: WriteMode::Append,
            expected: None,
            author: Author::anonymous(AuthorKind::User),
            now: "2026-11-21T19:00:00Z",
        })
        .unwrap();
    store
        .commit(workspace, notepad.revision(), &change)
        .unwrap();
}

#[test]
fn notes_survive_reopening_and_stay_within_their_workspace() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("notepads/notepads.sqlite3");
    let store = SqliteNotepadStore::open(&path).unwrap();
    assert_eq!(store.load("wks_a").unwrap(), Notepad::default());
    write(&store, "wks_a", "plan", "first");
    write(&store, "wks_a", "plan", "second");
    write(&store, "wks_b", "other", "text");
    let expected = store.load("wks_a").unwrap();
    drop(store);

    let reopened = SqliteNotepadStore::open(&path).unwrap();
    let loaded = reopened.load("wks_a").unwrap();
    assert_eq!(loaded, expected);
    assert_eq!(loaded.revision(), 2);
    assert_eq!(loaded.note("plan").unwrap().revision, 2);
    assert!(loaded.note("other").is_none());
    assert_eq!(reopened.load("wks_b").unwrap().revision(), 1);
}

#[test]
fn remove_and_clear_delete_notes() {
    let root = tempfile::tempdir().unwrap();
    let store = SqliteNotepadStore::open(&root.path().join("notepads.sqlite3")).unwrap();
    write(&store, "wks_a", "plan", "text");
    write(&store, "wks_a", "keep", "text");
    write(&store, "wks_b", "plan", "text");

    let mut notepad = store.load("wks_a").unwrap();
    let change = notepad.delete("plan", 1).unwrap();
    assert_eq!(change, NoteChange::Remove("plan".to_owned()));
    store.commit("wks_a", notepad.revision(), &change).unwrap();
    assert_eq!(store.load("wks_a").unwrap(), notepad);

    store.clear("wks_a").unwrap();
    assert_eq!(store.load("wks_a").unwrap(), Notepad::default());
    assert_eq!(store.load("wks_b").unwrap().notes().count(), 1);
}

#[test]
fn a_database_from_a_newer_version_or_with_invalid_notes_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("notepads.sqlite3");
    let store = SqliteNotepadStore::open(&path).unwrap();
    write(&store, "wks_a", "plan", "text");
    store
        .lock()
        .execute("UPDATE notes SET note = CAST('{' AS BLOB)", [])
        .unwrap();
    assert_eq!(store.load("wks_a"), Err(NotepadStoreError::Io));
    store.lock().pragma_update(None, "user_version", 2).unwrap();
    drop(store);
    assert_eq!(
        SqliteNotepadStore::open(&path).err(),
        Some(NotepadStoreError::Io)
    );
}

#[cfg(unix)]
#[test]
fn an_unwritable_directory_is_reported() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("file");
    std::fs::write(&file, b"").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        SqliteNotepadStore::open(&file.join("notepads.sqlite3")).err(),
        Some(NotepadStoreError::Io)
    );
}
