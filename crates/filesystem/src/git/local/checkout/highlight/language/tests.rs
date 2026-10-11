use super::{detect, modeline_language, syntax_for_filename, syntax_for_language};

#[test]
fn complete_filenames_and_variants_have_grammars_without_extensions() {
    for path in [
        "Dockerfile",
        "docker/Dockerfile.dev",
        "Makefile",
        "CMakeLists.txt",
        ".bashrc",
    ] {
        assert!(syntax_for_filename(path).is_some(), "{path}");
        assert!(detect(path, None, None).is_some(), "{path}");
    }
    assert!(syntax_for_filename("source.rs").is_none());
    assert!(detect("notes.txt", Some("unformatted text"), None).is_none());
}

#[test]
fn shebangs_identify_extensionless_files_and_override_extensions() {
    for (path, content, language) in [
        ("scripts/tool", "#!/usr/bin/env python3\nprint(1)", "python"),
        ("scripts/tool.rs", "#!/bin/bash\necho hello", "sh"),
        ("scripts/node", "#!/usr/bin/env node\nconst x = 1;", "js"),
    ] {
        assert_same_syntax(
            detect(path, Some(content), None),
            syntax_for_language(language),
        );
    }
}

#[test]
fn explicit_language_aliases_override_names_and_shebangs() {
    for alias in ["Rust", "RUST", "rs"] {
        assert_same_syntax(
            detect("script.py", Some("#!/bin/bash"), Some(alias)),
            syntax_for_language("rust"),
        );
    }
    assert_same_syntax(
        syntax_for_language("Shell-Script"),
        syntax_for_language("sh"),
    );
    assert_same_syntax(syntax_for_language("C#"), syntax_for_language("cs"));
    assert!(detect("source.rs", None, Some("Text")).is_none());
    assert!(detect("source.rs", None, Some("Unsupported Language")).is_none());
}

#[test]
fn vim_and_emacs_modelines_take_precedence_and_only_scan_file_edges() {
    for modeline in [
        "// vim: set ft=python:",
        "// -*- mode: python; -*-",
        "// -*- python -*-",
    ] {
        assert_eq!(modeline_language(modeline), Some("python"));
        assert_same_syntax(
            detect("source.rs", Some(modeline), None),
            syntax_for_language("python"),
        );
    }
    let trailer = format!("{}\n// vim: syntax=python:", "\n".repeat(10));
    assert_same_syntax(
        detect("source.rs", Some(&trailer), None),
        syntax_for_language("python"),
    );
    let middle = format!(
        "{}\n// vim: ft=python:\n{}",
        "\n".repeat(10),
        "\n".repeat(10)
    );
    assert_same_syntax(
        detect("source.rs", Some(&middle), None),
        syntax_for_language("rust"),
    );
    assert!(modeline_language("ordinary text").is_none());
}

fn assert_same_syntax(
    actual: Option<&super::SyntaxReference>,
    expected: Option<&super::SyntaxReference>,
) {
    assert_eq!(actual.unwrap().scope, expected.unwrap().scope);
}
