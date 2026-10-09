use super::*;
use crate::git::local::checkout::parse_diff;

fn parsed(path: &str, body: &str) -> ParsedDiffFile {
    parse_diff(&format!("diff --git a/{path} b/{path}\n{body}"))
        .pop()
        .unwrap()
}

fn roles(tokens: &[HighlightToken]) -> Vec<&str> {
    tokens
        .iter()
        .filter_map(|token| token.style.as_deref())
        .collect()
}

#[test]
fn common_languages_emit_client_syntax_roles_and_preserve_source() {
    for (path, code, expected) in [
        ("a.rs", "fn main() { let value = \"你好\"; }", "keyword"),
        ("a.ts", "const value: string = \"hello\";", "keyword"),
        ("a.tsx", "const view = <div title=\"hello\" />;", "tag"),
        ("a.py", "def greet(): return \"hello\"", "keyword"),
        ("a.go", "func main() { value := \"hello\" }", "keyword"),
        ("a.json", "{\"value\": 12}", "number"),
        ("a.yaml", "value: 12", "number"),
        ("a.md", "# Greeting", "heading"),
        ("a.mjs", "const value = true;", "literal"),
    ] {
        let syntax = syntax_for_path(path).unwrap();
        let tokens = tokenize_content(code, syntax).unwrap();
        assert!(
            roles(&tokens[0]).contains(&expected),
            "{path}: {:?}",
            tokens[0]
        );
        assert!(tokens_match(&tokens[0], code));
    }
    assert!(syntax_for_path("notes.unknownext").is_none());
    assert!(syntax_for_path("notes.txt").is_none());
    assert!(syntax_for_path("src/INDEX.TS").is_some());
}

#[test]
fn full_snapshots_supply_multiline_context_outside_diff_hunks() {
    let highlighter = DiffHighlighter::default();
    let mut file = parsed("a.ts", "@@ -7 +7 @@\n-old payload\n+new payload\n");
    let syntax = syntax_for_path(&file.path).unwrap();
    let old = "/*\nline 2\nline 3\nline 4\nline 5\nline 6\nold payload\n*/";
    let new = "const value = `\nline 2\nline 3\nline 4\nline 5\nline 6\nnew payload\n`;";
    let old_tokens = highlighter
        .lookup(&file, syntax, Some(old), DiffLineKind::Remove)
        .unwrap();
    let new_tokens = highlighter
        .lookup(&file, syntax, Some(new), DiffLineKind::Add)
        .unwrap();
    apply_tokens(&mut file, Some(&old_tokens), Some(&new_tokens));
    assert_eq!(
        roles(file.hunks[0].lines[1].tokens.as_ref().unwrap()),
        ["comment"]
    );
    assert_eq!(
        roles(file.hunks[0].lines[2].tokens.as_ref().unwrap()),
        ["string"]
    );
    assert!(file.hunks[0].lines[0].tokens.is_none());
}

#[test]
fn hunk_fallback_tracks_old_new_line_numbers_and_preserves_prefix_characters() {
    let mut file = parsed(
        "a.ts",
        "@@ -10,2 +10,3 @@\n-const oldValue = 1;\n+const newValue = 2;\n++newValue;\n  newValue;\n@@ -20 +21 @@\n-const x = 3;\n+const x = 4;\n",
    );
    let highlighter = DiffHighlighter::default();
    let syntax = syntax_for_path(&file.path).unwrap();
    let old_tokens = highlighter
        .lookup(&file, syntax, None, DiffLineKind::Remove)
        .unwrap();
    let new_tokens = highlighter
        .lookup(&file, syntax, None, DiffLineKind::Add)
        .unwrap();
    apply_tokens(&mut file, Some(&old_tokens), Some(&new_tokens));
    for line in file.hunks.iter().flat_map(|hunk| &hunk.lines) {
        if line.kind == DiffLineKind::Header {
            assert!(line.tokens.is_none());
        } else {
            assert!(tokens_match(line.tokens.as_ref().unwrap(), &line.content));
        }
    }
    assert_eq!(file.hunks[0].lines[3].content, "+newValue;");
    assert_eq!(file.hunks[0].lines[4].content, " newValue;");
    assert!(roles(file.hunks[1].lines[2].tokens.as_ref().unwrap()).contains(&"keyword"));
}

#[test]
fn mismatching_snapshots_cannot_replace_diff_content_or_offsets() {
    let mut file = parsed("a.rs", "@@ -1 +1 @@\n-old\n+new\n");
    let tokens = TokenLookup {
        first_line: 1,
        lines: Arc::new(vec![vec![HighlightToken {
            text: "concurrent write".to_owned(),
            style: Some("comment".to_owned()),
        }]]),
    };
    apply_tokens(&mut file, Some(&tokens), Some(&tokens));
    assert!(file.hunks[0].lines.iter().all(|line| line.tokens.is_none()));
    assert!(!tokens_match(&tokens.lines[0], "short"));
}

#[test]
fn cache_reuses_unchanged_content_and_invalidates_changed_content() {
    let highlighter = DiffHighlighter::default();
    let syntax = syntax_for_path("a.ts").unwrap();
    let first = highlighter.tokenize("const a = 1;", syntax).unwrap();
    let second = highlighter.tokenize("const a = 1;", syntax).unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    let changed = highlighter.tokenize("const a = 2;", syntax).unwrap();
    assert!(!Arc::ptr_eq(&first, &changed));
    assert!(tokens_match(&changed[0], "const a = 2;"));

    for index in 0..MAX_CACHE_ENTRIES {
        highlighter
            .tokenize(&format!("const a = {index};"), syntax)
            .unwrap();
    }
    assert_eq!(highlighter.cache.read().unwrap().len(), MAX_CACHE_ENTRIES);
    let other_syntax = syntax_for_path("a.rs").unwrap();
    let other = highlighter.tokenize("const a = 1;", other_syntax).unwrap();
    assert!(!Arc::ptr_eq(&first, &other));
}

#[test]
fn huge_lines_files_and_sparse_hunks_fall_back_without_unbounded_allocation() {
    let highlighter = DiffHighlighter::default();
    let syntax = syntax_for_path("a.ts").unwrap();
    assert!(
        highlighter
            .tokenize(&"x".repeat(MAX_LINE_CHARS + 1), syntax)
            .is_none()
    );
    assert!(
        highlighter
            .tokenize(&"x\n".repeat(MAX_FILE_BYTES), syntax)
            .is_none()
    );
    let sparse = parsed("a.ts", "@@ -1 +1 @@\n+x\n@@ -2000000 +2000000 @@\n+y\n");
    assert!(reconstruct(&sparse, DiffLineKind::Add).is_none());

    let mut unknown = parsed("a.unknownext", "@@ -1 +1 @@\n+a\n");
    highlighter.highlight(&mut unknown, Path::new("/unavailable"), "HEAD", None);
    assert!(unknown.hunks[0].lines[1].tokens.is_none());
    let mut huge = parsed(
        "a.ts",
        &format!("@@ -1 +1 @@\n+{}\n", "x".repeat(MAX_LINE_CHARS + 1)),
    );
    highlighter.highlight(&mut huge, Path::new("/unavailable"), "HEAD", None);
    assert!(huge.hunks[0].lines[1].tokens.is_none());
    assert!(highlighter.cache.read().unwrap().is_empty());
}

#[test]
fn unavailable_snapshots_use_hunks_and_invalid_paths_are_rejected() {
    let highlighter = DiffHighlighter::default();
    let mut file = parsed("a.ts", "@@ -1 +1 @@\n-const a = 1;\n+const a = 2;\n");
    highlighter.highlight(&mut file, Path::new("/unavailable"), "HEAD", Some("HEAD"));
    assert!(roles(file.hunks[0].lines[1].tokens.as_ref().unwrap()).contains(&"keyword"));
    assert!(roles(file.hunks[0].lines[2].tokens.as_ref().unwrap()).contains(&"keyword"));
    assert!(read_content(Path::new("/unavailable"), "../outside", None).is_none());
}

#[test]
fn empty_and_crlf_lines_preserve_geometry_without_newline_tokens() {
    let tokens = tokenize_content(
        "\r\nconst a = \"hello\";\r\n",
        syntax_for_path("a.ts").unwrap(),
    )
    .unwrap();
    assert_eq!(tokens.len(), 3);
    assert!(tokens[0].is_empty());
    assert!(tokens[2].is_empty());
    assert!(tokens_match(&tokens[1], "const a = \"hello\";"));
}

#[test]
fn template_interpolation_uses_inner_code_roles() {
    let code = "const text = `value: ${123}`;";
    let tokens = tokenize_content(code, syntax_for_path("a.ts").unwrap()).unwrap();
    assert!(
        tokens[0]
            .iter()
            .any(|token| token.text == "123" && token.style.as_deref() == Some("number"))
    );
    assert!(tokens_match(&tokens[0], code));
}
