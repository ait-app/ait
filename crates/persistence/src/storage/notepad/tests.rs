use std::fs;
use std::path::{Path, PathBuf};

use domain::workspace::notepad::Notepad;
use model::workspace::notepad::{NotepadStore, NotepadStoreError};

use super::{FileNotepadStore, MAX_FILE_BYTES, decode, encode};

struct Fixture {
    store: FileNotepadStore,
    root: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        Self {
            store: FileNotepadStore::new(root.path().join("notepads")),
            root,
        }
    }

    fn file(&self, workspace: &str) -> PathBuf {
        self.root
            .path()
            .join("notepads")
            .join(workspace)
            .join("notepad.md")
    }

    fn open(&self, workspace: &str) -> Notepad {
        self.store.open(workspace, None).unwrap()
    }

    fn save(&self, workspace: &str, text: &str) -> Notepad {
        self.store
            .save(workspace, &self.open(workspace), text)
            .unwrap()
    }
}

#[test]
fn a_notepad_is_an_empty_markdown_file_until_it_is_written() {
    let fixture = Fixture::new();
    let empty = fixture.open("wks_a");
    assert_eq!(Path::new(empty.path()), fixture.file("wks_a"));
    assert_eq!(empty.text(), "");
    assert_eq!(fs::read(fixture.file("wks_a")).unwrap(), b"");
    assert_eq!(fixture.open("wks_a"), empty);

    let written = fixture.save("wks_a", "# Notes\n");
    assert_eq!(written.text(), "# Notes\n");
    assert_ne!(written.revision(), empty.revision());
    assert_eq!(fs::read(fixture.file("wks_a")).unwrap(), b"# Notes\n");
    assert_eq!(fixture.open("wks_a"), written);
    assert_eq!(fixture.open("wks_b").text(), "");
    assert_eq!(
        fs::read_dir(fixture.file("wks_a").parent().unwrap())
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn the_revision_follows_the_text_whoever_wrote_the_file() {
    let fixture = Fixture::new();
    let first = fixture.save("wks_a", "one");
    fs::write(fixture.file("wks_a"), "\u{feff}edited").unwrap();
    let edited = fixture.open("wks_a");
    assert_eq!(edited.text(), "edited");
    assert_ne!(edited.revision(), first.revision());
    fs::write(fixture.file("wks_a"), "one").unwrap();
    assert_eq!(fixture.open("wks_a").revision(), first.revision());
}

#[test]
fn a_save_is_refused_once_the_file_no_longer_holds_what_was_read() {
    let fixture = Fixture::new();
    let read = fixture.open("wks_a");
    fs::write(fixture.file("wks_a"), "typed in the editor").unwrap();
    assert_eq!(
        fixture.store.save("wks_a", &read, "from an agent"),
        Err(NotepadStoreError::Changed)
    );
    assert_eq!(fixture.open("wks_a").text(), "typed in the editor");

    let read = fixture.open("wks_a");
    fs::remove_file(fixture.file("wks_a")).unwrap();
    assert_eq!(
        fixture.store.save("wks_a", &read, "from an agent"),
        Err(NotepadStoreError::Changed)
    );
    assert_eq!(fixture.open("wks_a").text(), "");
}

#[test]
fn a_new_notepad_starts_with_the_legacy_notebook_when_that_holds_text() {
    let fixture = Fixture::new();
    let legacy = fixture.root.path().join(".ait-notebook.md");
    fs::write(&legacy, "\u{feff}old notes\n").unwrap();
    let imported = fixture.store.open("wks_a", Some(&legacy)).unwrap();
    assert_eq!(imported.text(), "old notes\n");
    assert_eq!(fs::read_to_string(&legacy).unwrap(), "\u{feff}old notes\n");

    fs::write(&legacy, "changed later").unwrap();
    assert_eq!(
        fixture.store.open("wks_a", Some(&legacy)).unwrap(),
        imported
    );

    for unusable in [&b"  \n"[..], b"a\0b", b"\xff\xfe"] {
        fs::write(&legacy, unusable).unwrap();
        fixture.store.clear("wks_b").unwrap();
        assert_eq!(
            fixture.store.open("wks_b", Some(&legacy)).unwrap().text(),
            ""
        );
    }
    let missing = fixture.root.path().join("missing.md");
    assert_eq!(
        fixture.store.open("wks_c", Some(&missing)).unwrap().text(),
        ""
    );
}

#[test]
fn clear_removes_only_the_named_notepad() {
    let fixture = Fixture::new();
    fixture.save("wks_a", "text");
    fixture.save("wks_b", "text");
    fixture.store.clear("wks_a").unwrap();
    fixture.store.clear("wks_missing").unwrap();
    assert!(!fixture.file("wks_a").parent().unwrap().exists());
    assert_eq!(fixture.open("wks_b").text(), "text");
}

#[test]
fn workspace_identities_spell_one_directory_name_each() {
    for (id, name) in [
        ("wks_00ff", "wks_00ff"),
        ("Repo/main", "%52epo%2Fmain"),
        ("..", "%2E%2E"),
        ("é", "%C3%A9"),
    ] {
        assert_eq!(encode(id), name);
        assert_eq!(decode(name).as_deref(), Some(id));
    }
    for name in ["", "%", "%4", "%zz", "%61", "%2e", "A", "a.b", "%FF"] {
        assert_eq!(decode(name), None, "{name}");
    }
}

#[test]
fn the_owner_of_a_notepad_directory_is_its_workspace() {
    let fixture = Fixture::new();
    let root = fixture.root.path().join("notepads");
    assert_eq!(
        fixture.store.owner(&root.join("wks_a")).as_deref(),
        Some("wks_a")
    );
    assert_eq!(
        fixture.store.owner(&root.join("%52epo%2Fmain")).as_deref(),
        Some("Repo/main")
    );
    for foreign in [
        root.clone(),
        root.join("wks_a").join("nested"),
        root.join("wks_a").join(".."),
        root.join("Upper"),
        fixture.root.path().join("other").join("wks_a"),
    ] {
        assert_eq!(fixture.store.owner(&foreign), None, "{foreign:?}");
    }
}

#[test]
fn unusable_identities_and_files_are_storage_failures() {
    let fixture = Fixture::new();
    for id in [String::new(), "x".repeat(81)] {
        assert_eq!(fixture.store.open(&id, None), Err(NotepadStoreError::Io));
        assert_eq!(fixture.store.clear(&id), Err(NotepadStoreError::Io));
    }
    fixture.open("wks_a");
    fs::write(fixture.file("wks_a"), [0xff, 0xfe]).unwrap();
    assert_eq!(
        fixture.store.open("wks_a", None),
        Err(NotepadStoreError::Io)
    );
    let large = vec![b'x'; usize::try_from(MAX_FILE_BYTES).unwrap() + 1];
    fs::write(fixture.file("wks_a"), large).unwrap();
    assert_eq!(
        fixture.store.open("wks_a", None),
        Err(NotepadStoreError::Io)
    );
    fs::remove_file(fixture.file("wks_a")).unwrap();
    fs::create_dir(fixture.file("wks_a")).unwrap();
    assert_eq!(
        fixture.store.open("wks_a", None),
        Err(NotepadStoreError::Io)
    );
}
