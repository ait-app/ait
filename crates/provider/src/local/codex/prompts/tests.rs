use super::*;

#[test]
fn custom_prompts_keep_frontmatter_quoted_arguments_and_literal_dollars() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("review.md"), "---\ndescription: 'Review changes'\nargument-hint: [FILE]\n---\n$1 | $2 | $FILE | $FILE_MORE | $$FILE | $ARGUMENTS").unwrap();
    let commands = list(root.path()).unwrap();
    assert_eq!(commands[0]["name"], "prompts:review");
    assert_eq!(commands[0]["description"], "Review changes");
    assert_eq!(commands[0]["argumentHint"], "[FILE]");
    assert_eq!(
        invoke(
            root.path(),
            "review",
            "'hello world' second FILE='a b' FILE_MORE=more"
        )
        .unwrap(),
        "hello world | second | a b | more | $FILE | 'hello world' second FILE='a b' FILE_MORE=more"
    );
    assert_eq!(
        expand("$MISSING $1 $$ $1", "'value\\nnext'").unwrap(),
        "$MISSING value\nnext $ value\nnext"
    );
    assert!(invoke(root.path(), "../outside", "").is_err());
    assert!(invoke(root.path(), "missing", "").is_err());
    assert!(expand(&"$1".repeat(32768), "long value").is_err());
    assert!(list(&root.path().join("missing")).unwrap().is_empty());
}

#[test]
fn prompt_catalog_filters_noncommands_and_reads_plain_or_unclosed_frontmatter() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("nested.md")).unwrap();
    std::fs::write(root.path().join("ignored.txt"), "ignored").unwrap();
    std::fs::write(root.path().join("plain.md"), "plain $1").unwrap();
    std::fs::write(root.path().join("unclosed.md"), "---\ndescription: pending").unwrap();
    std::fs::write(
        root.path().join("comments.md"),
        "---\n# comment\nempty:\n: skipped\nargument_hint: FILE\n---\nbody",
    )
    .unwrap();

    let commands = list(root.path()).unwrap();

    assert_eq!(commands.len(), 3);
    assert_eq!(commands[0]["argumentHint"], "FILE");
    assert_eq!(commands[1]["name"], "prompts:plain");
    assert_eq!(commands[1]["description"], "Custom prompt");
    assert_eq!(
        invoke(root.path(), "plain", "fixture").unwrap(),
        "plain fixture"
    );
    assert_eq!(
        invoke(root.path(), "unclosed", "").unwrap(),
        "---\ndescription: pending"
    );
    assert_eq!(
        list(&root.path().join("ignored.txt")),
        Err(AgentSessionError::Failed)
    );
}

#[test]
fn prompt_expansion_bounds_input_and_preserves_quoted_escape_sequences() {
    assert_eq!(
        expand("$1", "\"tab\\tquote\\\"slash\\\\\"").unwrap(),
        "tab\tquote\"slash\\"
    );
    for (template, args) in [
        ("x".repeat(65537), String::new()),
        ("$1".into(), "x".repeat(65537)),
        ("$1".into(), "word ".repeat(130)),
    ] {
        assert_eq!(expand(&template, &args), Err(AgentSessionError::Rejected));
    }
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("oversized.md"), "x".repeat(65537)).unwrap();
    assert_eq!(
        invoke(root.path(), "oversized", ""),
        Err(AgentSessionError::Rejected)
    );
}
