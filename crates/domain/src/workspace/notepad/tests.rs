use super::{
    Author, AuthorKind, MAX_NOTES, MAX_TEXT_BYTES, NoteChange, Notepad, NotepadError, Write,
    WriteMode, valid_key,
};

const NOW: &str = "2026-11-21T19:00:00Z";

fn agent(id: &str) -> Author {
    Author {
        kind: AuthorKind::Agent,
        agent_id: Some(id.to_owned()),
        title: Some(format!("Agent {id}")),
    }
}

fn write<'a>(key: &'a str, text: &'a str, mode: WriteMode, expected: Option<u64>) -> Write<'a> {
    Write {
        key,
        text,
        mode,
        expected,
        author: agent("a"),
        now: NOW,
    }
}

#[test]
fn keys_are_lowercase_slugs() {
    for key in ["a", "api-contract", "0", &"k".repeat(64)] {
        assert!(valid_key(key), "{key}");
    }
    for key in ["", "-a", "A", "a b", "a_b", "é", &"k".repeat(65)] {
        assert!(!valid_key(key), "{key}");
    }
}

#[test]
fn authors_name_an_agent_only_as_agents() {
    assert!(agent("a").is_valid());
    assert!(Author::anonymous(AuthorKind::User).is_valid());
    assert!(!Author::anonymous(AuthorKind::Agent).is_valid());
    let mut bot = agent("a");
    bot.kind = AuthorKind::Bot;
    assert!(!bot.is_valid());
    let mut control = agent("a");
    control.title = Some("a\nb".to_owned());
    assert!(!control.is_valid());
    let mut long = agent("a");
    long.title = Some("t".repeat(129));
    assert!(!long.is_valid());
}

#[test]
fn replace_creates_then_requires_the_revision_read() {
    let mut notepad = Notepad::default();
    let NoteChange::Put(created) = notepad
        .write(write("plan", "first", WriteMode::Replace, None))
        .unwrap()
    else {
        panic!("expected a put");
    };
    assert_eq!((created.revision, created.notepad_revision), (1, 1));
    assert_eq!(created.created_by, agent("a"));

    let exists = notepad
        .write(write("plan", "second", WriteMode::Replace, None))
        .unwrap_err();
    assert!(matches!(exists, NotepadError::Exists(note) if note.revision == 1));
    let conflict = notepad
        .write(write("plan", "second", WriteMode::Replace, Some(7)))
        .unwrap_err();
    assert!(matches!(conflict, NotepadError::Conflict(note) if note.text == "first"));
    assert_eq!(notepad.revision(), 1);

    let mut replace = write("plan", "second", WriteMode::Replace, Some(1));
    replace.author = agent("b");
    replace.now = "2026-11-21T20:00:00Z";
    notepad.write(replace).unwrap();
    let note = notepad.note("plan").unwrap();
    assert_eq!((note.revision, note.notepad_revision), (2, 2));
    assert_eq!(note.text, "second");
    assert_eq!(
        (&note.created_by, &note.updated_by),
        (&agent("a"), &agent("b"))
    );
    assert_eq!(
        (note.created_at.as_str(), note.updated_at.as_str()),
        (NOW, "2026-11-21T20:00:00Z")
    );
}

#[test]
fn replace_with_a_revision_of_an_absent_note_is_missing() {
    let mut notepad = Notepad::default();
    assert_eq!(
        notepad.write(write("gone", "text", WriteMode::Replace, Some(1))),
        Err(NotepadError::Missing)
    );
}

#[test]
fn append_attributes_each_block_and_needs_no_revision() {
    let mut notepad = Notepad::default();
    notepad
        .write(write("questions", "Which port?", WriteMode::Append, None))
        .unwrap();
    let mut answer = write("questions", "7316", WriteMode::Append, None);
    answer.author = Author::anonymous(AuthorKind::Bot);
    notepad.write(answer).unwrap();
    let note = notepad.note("questions").unwrap();
    assert_eq!(
        note.text,
        format!("[Agent a · {NOW}]\nWhich port?\n\n[Workspace Bot · {NOW}]\n7316")
    );
    assert_eq!(note.revision, 2);
}

#[test]
fn author_labels_fall_back_by_kind() {
    let mut notepad = Notepad::default();
    let authors = [
        (
            Author {
                title: None,
                ..agent("agent-7")
            },
            "agent-7",
        ),
        (Author::anonymous(AuthorKind::User), "User"),
        (Author::anonymous(AuthorKind::External), "External caller"),
    ];
    for (index, (author, label)) in authors.into_iter().enumerate() {
        let key = format!("note-{index}");
        let mut append = write(&key, "text", WriteMode::Append, None);
        append.author = author;
        notepad.write(append).unwrap();
        assert!(
            notepad
                .note(&key)
                .unwrap()
                .text
                .starts_with(&format!("[{label} · "))
        );
    }
}

#[test]
fn invalid_input_and_limits_leave_the_notepad_unchanged() {
    let mut notepad = Notepad::default();
    let cases = [
        (
            write("Bad Key", "text", WriteMode::Replace, None),
            NotepadError::InvalidKey,
        ),
        (
            write("key", "", WriteMode::Replace, None),
            NotepadError::InvalidText,
        ),
        (
            write("key", "a\0b", WriteMode::Append, None),
            NotepadError::InvalidText,
        ),
    ];
    for (request, error) in cases {
        assert_eq!(notepad.write(request), Err(error));
    }
    let mut unnamed = write("key", "text", WriteMode::Replace, None);
    unnamed.author = Author::anonymous(AuthorKind::Agent);
    assert_eq!(notepad.write(unnamed), Err(NotepadError::InvalidText));

    let large = "x".repeat(MAX_TEXT_BYTES + 1);
    assert_eq!(
        notepad.write(write("key", &large, WriteMode::Replace, None)),
        Err(NotepadError::TooLarge)
    );
    let full = "x".repeat(MAX_TEXT_BYTES);
    notepad
        .write(write("key", &full, WriteMode::Replace, None))
        .unwrap();
    assert_eq!(
        notepad.write(write("key", "more", WriteMode::Append, None)),
        Err(NotepadError::TooLarge)
    );
    assert_eq!(notepad.revision(), 1);
}

#[test]
fn a_full_notepad_still_accepts_writes_to_existing_notes() {
    let mut notepad = Notepad::default();
    for index in 0..MAX_NOTES {
        notepad
            .write(write(
                &format!("note-{index}"),
                "text",
                WriteMode::Replace,
                None,
            ))
            .unwrap();
    }
    assert_eq!(
        notepad.write(write("one-more", "text", WriteMode::Append, None)),
        Err(NotepadError::Full)
    );
    notepad
        .write(write("note-0", "more", WriteMode::Append, None))
        .unwrap();
    assert_eq!(notepad.notes().count(), MAX_NOTES);
}

#[test]
fn delete_requires_the_current_revision() {
    let mut notepad = Notepad::default();
    notepad
        .write(write("plan", "text", WriteMode::Replace, None))
        .unwrap();
    assert_eq!(notepad.delete("absent", 1), Err(NotepadError::Missing));
    assert!(matches!(
        notepad.delete("plan", 2),
        Err(NotepadError::Conflict(_))
    ));
    assert_eq!(
        notepad.delete("plan", 1),
        Ok(NoteChange::Remove("plan".to_owned()))
    );
    assert_eq!(notepad.revision(), 2);
    assert!(notepad.note("plan").is_none());
}

#[test]
fn restore_rebuilds_the_stored_state() {
    let mut notepad = Notepad::default();
    notepad
        .write(write("b", "text", WriteMode::Replace, None))
        .unwrap();
    notepad
        .write(write("a", "text", WriteMode::Replace, None))
        .unwrap();
    let restored = Notepad::restore(notepad.revision(), notepad.notes().cloned());
    assert_eq!(restored, notepad);
    assert_eq!(
        restored
            .notes()
            .map(|note| note.key.as_str())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
}

#[test]
fn notes_use_camel_case_and_omit_absent_author_fields() {
    let mut notepad = Notepad::default();
    let mut request = write("plan", "text", WriteMode::Replace, None);
    request.author = Author::anonymous(AuthorKind::External);
    notepad.write(request).unwrap();
    let value = serde_json::to_value(notepad.note("plan").unwrap()).unwrap();
    assert_eq!(value["notepadRevision"], 1);
    assert_eq!(value["createdBy"], serde_json::json!({"kind":"external"}));
    assert_eq!(serde_json::to_value(WriteMode::Append).unwrap(), "append");
}
