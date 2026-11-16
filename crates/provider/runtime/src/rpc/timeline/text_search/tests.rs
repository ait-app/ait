use super::*;

#[test]
fn literal_queries_count_non_overlapping_case_insensitive_occurrences() {
    for (query, text, expected) in [
        ("a.b", "a.b axb A.B", 2),
        ("[x] (a)+", "[x] (a)+ [X]\t(A)+", 2),
        ("aa", "aaaaa", 2),
        ("hello world", "HELLO\n\tworld hello\u{a0}world", 2),
        ("σ", "Σ σ ς", 3),
        ("目标", "目标和目标", 2),
        ("hello", "hel\rlo", 1),
    ] {
        let pattern = pattern(query).unwrap().unwrap();
        assert_eq!(count(&pattern, text, false), expected, "{query}: {text}");
    }
}

#[test]
fn queries_are_bounded_after_whitespace_normalization() {
    assert!(pattern(" \n\t ").unwrap().is_none());
    assert!(pattern(&"x".repeat(4096)).unwrap().is_some());
    assert!(matches!(
        pattern(&"x".repeat(4097)),
        Err(ErrorCode::InvalidMessage)
    ));
}

#[test]
fn markdown_counts_visible_inline_text_entities_and_code() {
    for (query, text, expected) in [
        ("hello world", "hello **world**\n\nhello `world`", 2),
        ("hello & world", "hello &amp; **world**", 1),
        ("world next", "**world**\nnext line", 1),
        ("world next", "world  \nnext", 1),
        (
            "target",
            "# target\n\n> target\n\n- target\n- end target",
            4,
        ),
        ("a.b", "```ts\nconst value = 'a.b';\n```\n\n    a.b\n", 2),
        ("target", "[target](https://target.invalid \"target\")", 1),
        ("hidden", "![hidden **hidden**](https://hidden.invalid)", 0),
        ("target", "~~target~~ &lbrack;target&rbrack;", 2),
        ("<b>target</b>", "<b>target</b>", 1),
    ] {
        let pattern = pattern(query).unwrap().unwrap();
        assert_eq!(count(&pattern, text, true), expected, "{query}: {text}");
    }
}

#[test]
fn assistant_queries_never_cross_block_boundaries_or_search_hidden_syntax() {
    for text in [
        "one\n\ntarget",
        "- one\n- target",
        "# one\n\ntarget",
        "one\n\n> target",
        "```text\none\n```\n\ntarget",
        "| one | target |\n| --- | --- |\n| a | b |",
    ] {
        let pattern = pattern("one target").unwrap().unwrap();
        assert_eq!(count(&pattern, text, true), 0, "{text}");
    }
    let query = pattern("**target**").unwrap().unwrap();
    assert_eq!(count(&query, "**target**", true), 0);
    assert_eq!(count(&query, "**target**", false), 1);
    let query = pattern("one target").unwrap().unwrap();
    assert_eq!(count(&query, "one\n\ntarget", false), 1);
}
