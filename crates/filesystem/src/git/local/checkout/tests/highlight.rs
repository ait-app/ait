//! Real Git regressions for Paseo's snapshot-based diff highlighting behavior.

use std::fs;

use super::{Fixture, git, git_output};
use crate::git::local::checkout::LocalCheckout;
use crate::git::ports::checkout::{
    CheckoutDiffCompare, CheckoutDiffMode, CheckoutRuntime, DiffLineKind, ParsedDiffFile,
};

fn commit(fixture: &Fixture, path: &str, content: &str) -> String {
    fs::write(fixture.repo.join(path), content).unwrap();
    git(&fixture.repo, &["add", "--", path]);
    git(&fixture.repo, &["commit", "-m", "source"]);
    git_output(&fixture.repo, &["rev-parse", "HEAD"])
}

fn assert_role(file: &ParsedDiffFile, kind: DiffLineKind, role: &str) {
    let lines = file
        .hunks
        .iter()
        .flat_map(|hunk| &hunk.lines)
        .filter(|line| line.kind == kind);
    let mut count = 0;
    for line in lines {
        count += 1;
        let tokens = line.tokens.as_ref().unwrap();
        assert!(
            tokens
                .iter()
                .any(|token| token.style.as_deref() == Some(role)),
            "{line:?}"
        );
        assert_eq!(
            tokens
                .iter()
                .map(|token| token.text.as_str())
                .collect::<String>(),
            line.content
        );
    }
    assert!(count > 0);
}

#[test]
fn working_diff_highlights_old_new_and_context_lines_from_complete_files() {
    let fixture = Fixture::new();
    let original = format!(
        "/*\n{}\n*/\n",
        (1..14)
            .map(|i| format!("inside {i}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    commit(&fixture, "source.ts", &original);
    fs::write(
        fixture.repo.join("source.ts"),
        original.replace("inside 8", "changed 8"),
    )
    .unwrap();
    let runtime = LocalCheckout::new(fixture.temp.path().join("managed"));
    let result = runtime
        .diff(
            fixture.repo.to_str().unwrap(),
            &CheckoutDiffCompare {
                mode: CheckoutDiffMode::Uncommitted,
                base_ref: None,
                ignore_whitespace: false,
            },
        )
        .unwrap();
    let file = &result.files[0];
    assert!(
        file.hunks[0].old_start > 1,
        "opening comment lies outside the patch"
    );
    assert_role(file, DiffLineKind::Remove, "comment");
    assert_role(file, DiffLineKind::Add, "comment");
    assert_role(file, DiffLineKind::Context, "comment");
    assert!(file.hunks[0].lines[0].tokens.is_none());
}

#[test]
fn base_and_commit_diffs_highlight_the_requested_revision_instead_of_working_tree() {
    let fixture = Fixture::new();
    commit(&fixture, "source.ts", "const value = 'old';\n");
    git(&fixture.repo, &["checkout", "-b", "feature"]);
    let sha = commit(&fixture, "source.ts", "const value = 'committed';\n");
    fs::write(
        fixture.repo.join("source.ts"),
        "/* unrelated working tree */\n",
    )
    .unwrap();
    let runtime = LocalCheckout::new(fixture.temp.path().join("managed"));
    let base = runtime
        .diff(
            fixture.repo.to_str().unwrap(),
            &CheckoutDiffCompare {
                mode: CheckoutDiffMode::Base,
                base_ref: Some("main".to_owned()),
                ignore_whitespace: false,
            },
        )
        .unwrap();
    let committed = runtime
        .commit_file_diff(fixture.repo.to_str().unwrap(), &sha, "source.ts")
        .unwrap()
        .unwrap();
    for file in [&base.files[0], &committed] {
        assert_role(file, DiffLineKind::Remove, "keyword");
        assert_role(file, DiffLineKind::Add, "string");
        assert!(
            file.hunks[0]
                .lines
                .iter()
                .any(|line| line.content.contains("committed"))
        );
    }
}

#[test]
fn rename_uses_old_path_for_removed_line_context() {
    let fixture = Fixture::new();
    let original = format!(
        "/*\n{}\n*/\n",
        (1..14)
            .map(|i| format!("inside {i}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    commit(&fixture, "old.ts", &original);
    git(&fixture.repo, &["mv", "old.ts", "new.ts"]);
    fs::write(
        fixture.repo.join("new.ts"),
        original.replace("inside 8", "changed 8"),
    )
    .unwrap();
    let runtime = LocalCheckout::new(fixture.temp.path().join("managed"));
    let result = runtime
        .diff(
            fixture.repo.to_str().unwrap(),
            &CheckoutDiffCompare {
                mode: CheckoutDiffMode::Uncommitted,
                base_ref: None,
                ignore_whitespace: false,
            },
        )
        .unwrap();
    let file = &result.files[0];
    assert_eq!(file.old_path.as_deref(), Some("old.ts"));
    assert_role(file, DiffLineKind::Remove, "comment");
    assert_role(file, DiffLineKind::Add, "comment");
}

#[test]
fn deleted_and_untracked_files_keep_syntax_tokens() {
    let fixture = Fixture::new();
    commit(&fixture, "deleted.rs", "fn main() {}\n");
    fs::remove_file(fixture.repo.join("deleted.rs")).unwrap();
    fs::write(fixture.repo.join("added.tsx"), "const view = <div />;\n").unwrap();
    let runtime = LocalCheckout::new(fixture.temp.path().join("managed"));
    let result = runtime
        .diff(
            fixture.repo.to_str().unwrap(),
            &CheckoutDiffCompare {
                mode: CheckoutDiffMode::Uncommitted,
                base_ref: None,
                ignore_whitespace: false,
            },
        )
        .unwrap();
    assert_role(&result.files[0], DiffLineKind::Add, "tag");
    assert_role(&result.files[1], DiffLineKind::Remove, "keyword");
    assert!(result.files[0].is_new);
    assert!(result.files[1].is_deleted);
}

#[test]
fn cached_snapshots_follow_head_and_working_tree_changes() {
    let fixture = Fixture::new();
    commit(&fixture, "source.ts", "/* before */\n");
    fs::write(fixture.repo.join("source.ts"), "const value = 1;\n").unwrap();
    let runtime = LocalCheckout::new(fixture.temp.path().join("managed"));
    let compare = CheckoutDiffCompare {
        mode: CheckoutDiffMode::Uncommitted,
        base_ref: None,
        ignore_whitespace: false,
    };
    let first = runtime
        .diff(fixture.repo.to_str().unwrap(), &compare)
        .unwrap();
    assert_role(&first.files[0], DiffLineKind::Remove, "comment");
    assert_role(&first.files[0], DiffLineKind::Add, "keyword");
    let unchanged = runtime
        .diff(fixture.repo.to_str().unwrap(), &compare)
        .unwrap();
    assert_eq!(first, unchanged);

    commit(&fixture, "source.ts", "const value = 1;\n");
    fs::write(fixture.repo.join("source.ts"), "/* after */\n").unwrap();
    let next = runtime
        .diff(fixture.repo.to_str().unwrap(), &compare)
        .unwrap();
    assert_role(&next.files[0], DiffLineKind::Remove, "keyword");
    assert_role(&next.files[0], DiffLineKind::Add, "comment");
}
