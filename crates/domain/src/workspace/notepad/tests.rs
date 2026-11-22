use super::{Author, AuthorKind, MAX_TEXT_BYTES, Notepad, NotepadError, Write, WriteMode};

const NOW: &str = "2026-11-21T19:00:00Z";

fn agent(id: &str) -> Author {
    Author {
        kind: AuthorKind::Agent,
        agent_id: Some(id.to_owned()),
        title: Some(format!("Agent {id}")),
    }
}

/// A notepad file holding `text` at revision `r1`.
fn notepad(text: &str) -> Notepad {
    Notepad::new(
        "/data/notepads/wks_a/notepad.md".to_owned(),
        text.to_owned(),
        "r1".to_owned(),
        NOW.to_owned(),
    )
}

fn write<'a>(text: &'a str, mode: WriteMode, expected: Option<&'a str>) -> Write<'a> {
    Write {
        text,
        mode,
        expected,
        author: agent("a"),
        now: NOW,
    }
}

#[test]
fn a_notepad_reports_its_file_text_and_revision() {
    let notepad = notepad("text");
    assert_eq!(notepad.path(), "/data/notepads/wks_a/notepad.md");
    assert_eq!((notepad.text(), notepad.revision()), ("text", "r1"));
    assert_eq!(notepad.updated_at(), NOW);
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
fn replace_requires_the_revision_read_once_the_notepad_holds_text() {
    let written = notepad("first");
    for stale in [None, Some(""), Some("r0")] {
        assert_eq!(
            written.written(&write("second", WriteMode::Replace, stale)),
            Err(NotepadError::Conflict),
            "{stale:?}"
        );
    }
    assert_eq!(
        written.written(&write("second", WriteMode::Replace, Some("r1"))),
        Ok("second".to_owned())
    );
    assert_eq!(
        written.written(&write("", WriteMode::Replace, Some("r1"))),
        Ok(String::new())
    );
}

#[test]
fn an_empty_notepad_is_replaced_without_a_revision_or_with_its_own() {
    let empty = notepad("");
    for expected in [None, Some("r1")] {
        assert_eq!(
            empty.written(&write("text", WriteMode::Replace, expected)),
            Ok("text".to_owned()),
            "{expected:?}"
        );
    }
    assert_eq!(
        empty.written(&write("text", WriteMode::Replace, Some("r0"))),
        Err(NotepadError::Conflict)
    );
}

#[test]
fn append_attributes_each_block_and_needs_no_revision() {
    let question = notepad("")
        .written(&write("Which port?\n", WriteMode::Append, None))
        .unwrap();
    assert_eq!(question, format!("[Agent a · {NOW}]\nWhich port?\n"));
    let mut answer = write("7316", WriteMode::Append, Some("stale"));
    answer.author = Author::anonymous(AuthorKind::Bot);
    assert_eq!(
        notepad(&question).written(&answer).unwrap(),
        format!("{question}\n[Workspace Bot · {NOW}]\n7316\n")
    );
}

#[test]
fn an_appended_block_is_separated_from_the_text_by_one_blank_line() {
    let block = format!("[Agent a · {NOW}]\nnote\n");
    for (text, separator) in [("typed", "\n\n"), ("typed\n", "\n"), ("typed\n\n", "")] {
        assert_eq!(
            notepad(text)
                .written(&write("note", WriteMode::Append, None))
                .unwrap(),
            format!("{text}{separator}{block}"),
            "{text:?}"
        );
    }
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
        let mut append = write("text", WriteMode::Append, None);
        append.author = author;
        let text = notepad("").written(&append).unwrap();
        assert!(text.starts_with(&format!("[{label} · ")), "{text}");
    }
}

#[test]
fn invalid_input_and_the_size_limit_are_refused() {
    let empty = notepad("");
    assert_eq!(
        empty.written(&write("a\0b", WriteMode::Replace, None)),
        Err(NotepadError::Invalid)
    );
    for blank in ["", "\n\n"] {
        assert_eq!(
            empty.written(&write(blank, WriteMode::Append, None)),
            Err(NotepadError::Invalid),
            "{blank:?}"
        );
    }
    let mut unnamed = write("text", WriteMode::Replace, None);
    unnamed.author = Author::anonymous(AuthorKind::Agent);
    assert_eq!(empty.written(&unnamed), Err(NotepadError::Invalid));

    let large = "x".repeat(MAX_TEXT_BYTES + 1);
    assert_eq!(
        empty.written(&write(&large, WriteMode::Replace, None)),
        Err(NotepadError::TooLarge)
    );
    let full = &large[..MAX_TEXT_BYTES];
    assert_eq!(
        empty.written(&write(full, WriteMode::Replace, None)),
        Ok(full.to_owned())
    );
    assert_eq!(
        notepad(full).written(&write("more", WriteMode::Append, None)),
        Err(NotepadError::TooLarge)
    );
}

#[test]
fn write_modes_use_snake_case() {
    assert_eq!(serde_json::to_value(WriteMode::Append).unwrap(), "append");
    assert_eq!(
        serde_json::from_value::<WriteMode>(serde_json::json!("replace")).unwrap(),
        WriteMode::Replace
    );
}
