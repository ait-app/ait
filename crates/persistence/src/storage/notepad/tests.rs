use domain::workspace::notepad::{Author, AuthorKind, Notepad, Write, WriteMode};
use model::workspace::notepad::{NotepadStore, NotepadStoreError};

use super::SqliteNotepadStore;

fn append(store: &SqliteNotepadStore, workspace: &str, text: &str) -> Notepad {
    let mut notepad = store.load(workspace).unwrap();
    notepad
        .write(Write {
            text,
            mode: WriteMode::Append,
            expected: None,
            author: Author::anonymous(AuthorKind::User),
            now: "2026-11-21T19:00:00Z",
        })
        .unwrap();
    store.save(workspace, &notepad).unwrap();
    notepad
}

#[test]
fn notepads_survive_reopening_and_stay_within_their_workspace() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("notepads/notepads.sqlite3");
    let store = SqliteNotepadStore::open(&path).unwrap();
    assert_eq!(store.load("wks_a").unwrap(), Notepad::default());
    append(&store, "wks_a", "first");
    let expected = append(&store, "wks_a", "second");
    append(&store, "wks_b", "other");
    drop(store);

    let reopened = SqliteNotepadStore::open(&path).unwrap();
    let loaded = reopened.load("wks_a").unwrap();
    assert_eq!(loaded, expected);
    assert_eq!(loaded.revision(), 2);
    assert!(!loaded.text().contains("other"));
    assert_eq!(reopened.load("wks_b").unwrap().revision(), 1);
}

#[test]
fn clear_removes_only_the_named_notepad() {
    let root = tempfile::tempdir().unwrap();
    let store = SqliteNotepadStore::open(&root.path().join("notepads.sqlite3")).unwrap();
    append(&store, "wks_a", "text");
    append(&store, "wks_b", "text");
    store.clear("wks_a").unwrap();
    store.clear("wks_missing").unwrap();
    assert_eq!(store.load("wks_a").unwrap(), Notepad::default());
    assert_eq!(store.load("wks_b").unwrap().revision(), 1);
}

#[test]
fn a_database_from_a_newer_version_or_with_an_invalid_notepad_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("notepads.sqlite3");
    let store = SqliteNotepadStore::open(&path).unwrap();
    append(&store, "wks_a", "text");
    store
        .lock()
        .execute("UPDATE notepads SET notepad = CAST('{' AS BLOB)", [])
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
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("file");
    std::fs::write(&file, b"").unwrap();
    assert_eq!(
        SqliteNotepadStore::open(&file.join("notepads.sqlite3")).err(),
        Some(NotepadStoreError::Io)
    );
}
