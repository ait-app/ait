use super::*;

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
