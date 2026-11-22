use super::{Author, AuthorKind, MAX_TEXT_BYTES, Notepad, NotepadError, Write, WriteMode};

const NOW: &str = "2026-11-21T19:00:00Z";

fn agent(id: &str) -> Author {
    Author {
        kind: AuthorKind::Agent,
        agent_id: Some(id.to_owned()),
        title: Some(format!("Agent {id}")),
    }
}

fn write(text: &str, mode: WriteMode, expected: Option<u64>) -> Write<'_> {
    Write {
        text,
        mode,
        expected,
        author: agent("a"),
        now: NOW,
    }
}

#[test]
fn a_workspace_always_has_a_notepad_that_starts_empty() {
    let notepad = Notepad::default();
    assert_eq!((notepad.text(), notepad.revision()), ("", 0));
    assert_eq!((notepad.updated_by(), notepad.updated_at()), (None, None));
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
fn replace_requires_the_revision_read_once_the_notepad_was_written() {
    let mut notepad = Notepad::default();
    notepad
        .write(write("first", WriteMode::Replace, None))
        .unwrap();
    assert_eq!((notepad.text(), notepad.revision()), ("first", 1));
    assert_eq!(notepad.updated_by(), Some(&agent("a")));
    assert_eq!(notepad.updated_at(), Some(NOW));

    for stale in [None, Some(0), Some(7)] {
        assert_eq!(
            notepad.write(write("second", WriteMode::Replace, stale)),
            Err(NotepadError::Conflict),
            "{stale:?}"
        );
    }
    assert_eq!((notepad.text(), notepad.revision()), ("first", 1));

    let mut replace = write("", WriteMode::Replace, Some(1));
    replace.author = Author::anonymous(AuthorKind::User);
    replace.now = "2026-11-21T20:00:00Z";
    notepad.write(replace).unwrap();
    assert_eq!((notepad.text(), notepad.revision()), ("", 2));
    assert_eq!(
        notepad.updated_by(),
        Some(&Author::anonymous(AuthorKind::User))
    );
    assert_eq!(notepad.updated_at(), Some("2026-11-21T20:00:00Z"));
}

#[test]
fn a_never_written_notepad_is_replaced_at_revision_zero() {
    let mut explicit = Notepad::default();
    explicit
        .write(write("text", WriteMode::Replace, Some(0)))
        .unwrap();
    assert_eq!(explicit.revision(), 1);
    let mut stale = Notepad::default();
    assert_eq!(
        stale.write(write("text", WriteMode::Replace, Some(1))),
        Err(NotepadError::Conflict)
    );
}

#[test]
fn append_attributes_each_block_and_needs_no_revision() {
    let mut notepad = Notepad::default();
    notepad
        .write(write("Which port?", WriteMode::Append, None))
        .unwrap();
    let mut answer = write("7316", WriteMode::Append, Some(99));
    answer.author = Author::anonymous(AuthorKind::Bot);
    notepad.write(answer).unwrap();
    assert_eq!(
        notepad.text(),
        format!("[Agent a · {NOW}]\nWhich port?\n\n[Workspace Bot · {NOW}]\n7316")
    );
    assert_eq!(notepad.revision(), 2);
}

#[test]
fn author_labels_fall_back_by_kind() {
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
    for (author, label) in authors {
        let mut notepad = Notepad::default();
        let mut append = write("text", WriteMode::Append, None);
        append.author = author;
        notepad.write(append).unwrap();
        assert!(notepad.text().starts_with(&format!("[{label} · ")));
    }
}

#[test]
fn invalid_input_and_the_size_limit_leave_the_notepad_unchanged() {
    let mut notepad = Notepad::default();
    assert_eq!(
        notepad.write(write("a\0b", WriteMode::Replace, None)),
        Err(NotepadError::Invalid)
    );
    assert_eq!(
        notepad.write(write("", WriteMode::Append, None)),
        Err(NotepadError::Invalid)
    );
    let mut unnamed = write("text", WriteMode::Replace, None);
    unnamed.author = Author::anonymous(AuthorKind::Agent);
    assert_eq!(notepad.write(unnamed), Err(NotepadError::Invalid));

    let large = "x".repeat(MAX_TEXT_BYTES + 1);
    assert_eq!(
        notepad.write(write(&large, WriteMode::Replace, None)),
        Err(NotepadError::TooLarge)
    );
    notepad
        .write(write(&large[..MAX_TEXT_BYTES], WriteMode::Replace, None))
        .unwrap();
    assert_eq!(
        notepad.write(write("more", WriteMode::Append, None)),
        Err(NotepadError::TooLarge)
    );
    assert_eq!(notepad.revision(), 1);
}

#[test]
fn notepads_use_camel_case_and_omit_what_was_never_written() {
    let mut notepad = Notepad::default();
    assert_eq!(
        serde_json::to_value(&notepad).unwrap(),
        serde_json::json!({"text":"","revision":0})
    );
    let mut request = write("text", WriteMode::Replace, None);
    request.author = Author::anonymous(AuthorKind::External);
    notepad.write(request).unwrap();
    let value = serde_json::to_value(&notepad).unwrap();
    assert_eq!(value["updatedBy"], serde_json::json!({"kind":"external"}));
    assert_eq!(value["updatedAt"], NOW);
    assert_eq!(serde_json::from_value::<Notepad>(value).unwrap(), notepad);
    assert_eq!(serde_json::to_value(WriteMode::Append).unwrap(), "append");
}
