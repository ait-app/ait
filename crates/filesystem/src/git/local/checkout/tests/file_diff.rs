use super::*;

#[test]
fn committed_renames_and_subdirectory_reads_keep_repository_paths() {
    let fixture = Fixture::new();
    std::fs::create_dir(fixture.repo.join("nested")).unwrap();
    let content = "stable original line\n".repeat(30);
    std::fs::write(fixture.repo.join("old.txt"), &content).unwrap();
    git(&fixture.repo, &["add", "-A"]);
    git(&fixture.repo, &["commit", "-m", "original"]);
    git(&fixture.repo, &["mv", "old.txt", "new.txt"]);
    std::fs::write(
        fixture.repo.join("new.txt"),
        format!("{content}last line\n"),
    )
    .unwrap();
    git(&fixture.repo, &["commit", "-am", "rename and edit"]);
    let sha = git_output(&fixture.repo, &["rev-parse", "HEAD"]);
    let runtime = LocalCheckout::new(fixture.temp.path().join("managed"));
    let file = runtime
        .commit_file_diff(
            fixture.repo.join("nested").to_str().unwrap(),
            &sha,
            "new.txt",
        )
        .unwrap()
        .unwrap();
    assert_eq!(file.old_path.as_deref(), Some("old.txt"));
    assert_eq!(file.additions, 1);
    assert!(!file.is_new);
    std::fs::write(fixture.repo.join("new.txt"), "temporary edit\n").unwrap();
    runtime
        .discard_changes(
            fixture.repo.join("nested").to_str().unwrap(),
            &["new.txt".to_owned()],
        )
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(fixture.repo.join("new.txt")).unwrap(),
        format!("{content}last line\n")
    );
}

#[test]
#[cfg(unix)]
fn quoted_filenames_remain_addressable_in_commit_lists_and_patches() {
    let fixture = Fixture::new();
    let runtime = LocalCheckout::new(fixture.temp.path().join("managed"));
    for path in [
        "quote\"file.txt",
        "tab\tfile.txt",
        "new\nline.txt",
        "slash\\file.txt",
    ] {
        std::fs::write(fixture.repo.join(path), "quoted file\n").unwrap();
    }
    git(&fixture.repo, &["add", "-A"]);
    git(&fixture.repo, &["commit", "-m", "quoted paths"]);
    let sha = git_output(&fixture.repo, &["rev-parse", "HEAD"]);
    let commits = runtime.commits(fixture.repo.to_str().unwrap()).unwrap();
    for path in [
        "quote\"file.txt",
        "tab\tfile.txt",
        "new\nline.txt",
        "slash\\file.txt",
    ] {
        let file = runtime
            .commit_file_diff(fixture.repo.to_str().unwrap(), &sha, path)
            .unwrap()
            .unwrap();
        assert_eq!(file.path, path);
        assert_eq!(file.additions, 1);
        assert!(
            commits
                .commits
                .iter()
                .any(|commit| commit.files.iter().any(|file| file.path == path))
        );
    }
}

#[test]
#[cfg(unix)]
fn commit_file_diff_treats_metacharacters_as_literal_file_names() {
    let fixture = Fixture::new();
    for path in [
        "[page].txt",
        "p.txt",
        ":(literal)literal.txt",
        "literal.txt",
    ] {
        std::fs::write(fixture.repo.join(path), "literal file\n").unwrap();
    }
    git(&fixture.repo, &["add", "-A"]);
    git(&fixture.repo, &["commit", "-m", "special file names"]);
    let sha = git_output(&fixture.repo, &["rev-parse", "HEAD"]);
    let runtime = LocalCheckout::new(fixture.temp.path().join("managed"));

    for path in ["[page].txt", ":(literal)literal.txt"] {
        let file = runtime
            .commit_file_diff(fixture.repo.to_str().unwrap(), &sha, path)
            .unwrap()
            .expect("literal file should have a diff");
        assert_eq!(file.path, path);
        assert_eq!(file.additions, 1);
    }
}

#[test]
fn binary_marker_in_text_does_not_hide_a_commit_diff() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.repo.join("tracked.txt"),
        "Binary files are documented here\n",
    )
    .unwrap();
    git(&fixture.repo, &["add", "tracked.txt"]);
    git(&fixture.repo, &["commit", "-m", "document binary files"]);
    let sha = git_output(&fixture.repo, &["rev-parse", "HEAD"]);
    let runtime = LocalCheckout::new(fixture.temp.path().join("managed"));

    let file = runtime
        .commit_file_diff(fixture.repo.to_str().unwrap(), &sha, "tracked.txt")
        .unwrap()
        .expect("text mentioning binary files should remain visible");
    assert_eq!(file.additions, 1);
    assert_eq!(file.deletions, 1);
    assert!(file.status.is_none());
}
