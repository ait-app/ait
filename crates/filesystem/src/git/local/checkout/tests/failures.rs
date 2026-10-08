use super::*;

#[test]
fn checkout_directory_aliases_expand_consistently_and_files_are_rejected() {
    use crate::git::local::checkout::expanded_directory;
    let home = std::env::var("HOME").unwrap();
    let expected = Path::new(&home).canonicalize().unwrap();
    assert_eq!(expanded_directory("~").unwrap(), expected);
    assert_eq!(expanded_directory("~/.").unwrap(), expected);
    let fixture = Fixture::new();
    assert_eq!(
        expanded_directory(fixture.repo.join("tracked.txt").to_str().unwrap())
            .unwrap_err()
            .kind,
        CheckoutFailureKind::NotAllowed
    );
}

#[test]
fn invalid_revisions_cannot_be_interpreted_as_git_options_or_paths() {
    let fixture = Fixture::new();
    let runtime = LocalCheckout::new(fixture.temp.path().join("managed"));
    let cwd = fixture.repo.to_str().unwrap();
    for reference in [
        "--output=unsafe",
        "main\nHEAD",
        "main HEAD",
        &"x".repeat(257),
    ] {
        let compare = CheckoutDiffCompare {
            mode: CheckoutDiffMode::Base,
            base_ref: Some(reference.to_owned()),
            ignore_whitespace: false,
        };
        assert_eq!(
            runtime.diff(cwd, &compare).unwrap_err().kind,
            CheckoutFailureKind::NotAllowed
        );
    }
    assert!(git_output(&fixture.repo, &["status", "--porcelain"]).is_empty());
    for sha in ["HEAD", "--help", "abc", &"f".repeat(65)] {
        assert_eq!(
            runtime
                .commit_file_diff(cwd, sha, "tracked.txt")
                .unwrap_err()
                .kind,
            CheckoutFailureKind::NotAllowed
        );
    }
    for branch in ["HEAD", "origin", "refs/heads/"] {
        assert_eq!(
            runtime.validate_branch(cwd, branch).unwrap(),
            CheckoutBranchResolution::NotFound
        );
    }
}

#[test]
fn tracked_untracked_and_combined_diff_budgets_return_an_explicit_limit_result() {
    let fixture = Fixture::new();
    let runtime = LocalCheckout::new(fixture.temp.path().join("managed"));
    let cwd = fixture.repo.to_str().unwrap();
    let compare = CheckoutDiffCompare {
        mode: CheckoutDiffMode::Uncommitted,
        base_ref: None,
        ignore_whitespace: true,
    };
    let large = "x".repeat(4 * 1024 * 1024 + 1);
    for file in ["tracked.txt", "untracked.txt"] {
        std::fs::write(fixture.repo.join(file), &large).unwrap();
        let diff = runtime.diff(cwd, &compare).unwrap();
        assert!(diff.diff_too_large, "{file}");
        assert!(diff.files.is_empty());
        if file == "tracked.txt" {
            git(&fixture.repo, &["checkout", "--", file]);
        } else {
            std::fs::remove_file(fixture.repo.join(file)).unwrap();
        }
    }
    for file in ["first.txt", "second.txt"] {
        std::fs::write(fixture.repo.join(file), "x".repeat(2 * 1024 * 1024)).unwrap();
    }
    assert!(runtime.diff(cwd, &compare).unwrap().diff_too_large);
    std::fs::remove_file(fixture.repo.join("second.txt")).unwrap();
    let recovered = runtime.diff(cwd, &compare).unwrap();
    assert!(!recovered.diff_too_large);
    assert_eq!(recovered.files.len(), 1);
}

#[test]
fn rejected_mutations_leave_the_checkout_head_and_content_untouched() {
    let fixture = Fixture::new();
    let runtime = LocalCheckout::new(fixture.temp.path().join("managed"));
    let cwd = fixture.repo.to_str().unwrap();
    let before = git_output(&fixture.repo, &["rev-parse", "HEAD"]);
    assert!(runtime.commit(cwd, "  ", true).is_err());
    assert!(runtime.discard_changes(cwd, &[]).is_err());
    for branch in [
        "",
        "-leading",
        "trailing-",
        "two--hyphens",
        &"a".repeat(101),
    ] {
        assert!(runtime.rename_branch(cwd, branch).is_err(), "{branch}");
    }
    runtime
        .merge_to_base(cwd, Some("main"), CheckoutMergeStrategy::Merge, true)
        .unwrap();
    runtime.merge_from_base(cwd, Some("main"), true).unwrap();
    for base in ["absent", "refs/remotes/upstream/main", "refs/heads/absent"] {
        assert!(
            runtime
                .merge_to_base(cwd, Some(base), CheckoutMergeStrategy::Merge, true)
                .is_err()
        );
        assert!(runtime.merge_from_base(cwd, Some(base), true).is_err());
    }
    assert_eq!(git_output(&fixture.repo, &["rev-parse", "HEAD"]), before);
    git(&fixture.repo, &["checkout", "--detach"]);
    let history = runtime.commits(cwd).unwrap();
    assert!(history.base_ref.is_none());
    assert!(history.commits.is_empty());
    assert!(runtime.rename_branch(cwd, "new-name").is_err());
    assert!(runtime.pull(cwd).is_err());
    assert!(runtime.push(cwd).is_err());
    assert_eq!(git_output(&fixture.repo, &["rev-parse", "HEAD"]), before);
    assert_eq!(
        std::fs::read_to_string(fixture.repo.join("tracked.txt")).unwrap(),
        "one\n"
    );
}

#[test]
fn unborn_checkout_discard_unstages_new_files_without_deleting_the_worktree() {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-b", "main"]);
    let runtime = LocalCheckout::new(root.path().join("managed"));
    let cwd = root.path().to_str().unwrap();
    assert!(runtime.commits(cwd).unwrap().commits.is_empty());
    std::fs::write(root.path().join("new.txt"), "not committed").unwrap();
    git(root.path(), &["add", "new.txt"]);
    runtime
        .discard_changes(cwd, &["new.txt".to_owned()])
        .unwrap();
    assert!(git_output(root.path(), &["ls-files"]).is_empty());
    assert!(!root.path().join("new.txt").exists());
    assert!(root.path().join(".git").is_dir());
}

#[test]
fn failed_pull_clears_merge_state_and_preserves_local_commit() {
    let fixture = Fixture::new();
    let remote = fixture.temp.path().join("remote.git");
    git(
        fixture.temp.path(),
        &["init", "--bare", remote.to_str().unwrap()],
    );
    git(
        &fixture.repo,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&fixture.repo, &["push", "-u", "origin", "main"]);
    let clone = fixture.temp.path().join("clone");
    git(
        fixture.temp.path(),
        &[
            "clone",
            "-b",
            "main",
            remote.to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    );
    git(&clone, &["config", "user.name", "Fixture"]);
    git(&clone, &["config", "user.email", "fixture@example.test"]);
    std::fs::write(clone.join("tracked.txt"), "remote edit\n").unwrap();
    git(&clone, &["commit", "-am", "remote change"]);
    git(&clone, &["push"]);
    std::fs::write(fixture.repo.join("tracked.txt"), "local edit\n").unwrap();
    git(&fixture.repo, &["commit", "-am", "local change"]);
    git(&fixture.repo, &["config", "pull.rebase", "false"]);
    let before = git_output(&fixture.repo, &["rev-parse", "HEAD"]);
    let runtime = LocalCheckout::new(fixture.temp.path().join("managed"));
    assert!(runtime.pull(fixture.repo.to_str().unwrap()).is_err());
    assert_eq!(git_output(&fixture.repo, &["rev-parse", "HEAD"]), before);
    assert_eq!(
        std::fs::read_to_string(fixture.repo.join("tracked.txt")).unwrap(),
        "local edit\n"
    );
    assert!(!fixture.repo.join(".git/MERGE_HEAD").exists());
    assert!(git_output(&fixture.repo, &["status", "--porcelain"]).is_empty());
}
