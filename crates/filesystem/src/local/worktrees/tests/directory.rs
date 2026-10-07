use model::workspace::worktrees::DirectoryGit;

use super::*;

#[test]
fn source_branch_creation_has_no_upstream_and_checkout_requires_a_clean_directory() {
    let fixture = Fixture::new();
    let adapter = fixture.adapter();
    let cwd = path(&fixture.repository);
    run(
        &fixture.repository,
        &["config", "branch.autoSetupMerge", "always"],
    );
    adapter
        .prepare_directory(
            &cwd,
            &DirectoryGit::BranchOff {
                branch: "feature".into(),
                base: Some("main".into()),
            },
        )
        .unwrap();
    assert_eq!(branch(&fixture.repository), "feature");
    assert!(
        optional_git(
            &fixture.repository,
            &["config", "--get", "branch.feature.remote"]
        )
        .is_none()
    );
    std::fs::write(fixture.repository.join("README.md"), "dirty").unwrap();
    assert!(
        adapter
            .prepare_directory(
                &cwd,
                &DirectoryGit::Checkout {
                    branch: "main".into()
                }
            )
            .is_err()
    );
    assert_eq!(branch(&fixture.repository), "feature");
    run(&fixture.repository, &["restore", "README.md"]);
    adapter
        .prepare_directory(
            &cwd,
            &DirectoryGit::Checkout {
                branch: "main".into(),
            },
        )
        .unwrap();
    assert_eq!(branch(&fixture.repository), "main");
    for (branch, base) in [
        ("feature", "main"),
        ("bad branch", "main"),
        ("new", "main..HEAD"),
        ("new", "missing"),
    ] {
        assert!(
            adapter
                .prepare_directory(
                    &cwd,
                    &DirectoryGit::BranchOff {
                        branch: branch.into(),
                        base: Some(base.into())
                    }
                )
                .is_err()
        );
    }
    assert_eq!(branch(&fixture.repository), "main");
}

#[test]
fn legacy_directory_checkout_resolves_remote_tracking_and_default_base() {
    let fixture = Fixture::new();
    let adapter = fixture.adapter();
    let cwd = path(&fixture.repository);
    run(
        &fixture.repository,
        &[
            "remote",
            "add",
            "origin",
            "https://example.invalid/repo.git",
        ],
    );
    run(
        &fixture.repository,
        &["update-ref", "refs/remotes/origin/release", "HEAD"],
    );
    adapter
        .prepare_directory(
            &cwd,
            &DirectoryGit::Checkout {
                branch: "origin/release".into(),
            },
        )
        .unwrap();
    assert_eq!(branch(&fixture.repository), "release");
    assert_eq!(
        optional_git(
            &fixture.repository,
            &["rev-parse", "--abbrev-ref", "@{upstream}"]
        )
        .as_deref(),
        Some("origin/release")
    );
    adapter
        .prepare_directory(
            &cwd,
            &DirectoryGit::BranchOff {
                branch: "default-base".into(),
                base: None,
            },
        )
        .unwrap();
    assert_eq!(branch(&fixture.repository), "default-base");
    for name in ["--help", "missing", "HEAD", "a@{b}"] {
        assert!(
            adapter
                .prepare_directory(
                    &cwd,
                    &DirectoryGit::Checkout {
                        branch: name.into()
                    }
                )
                .is_err()
        );
    }
}
