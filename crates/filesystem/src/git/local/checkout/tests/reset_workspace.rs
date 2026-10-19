use std::path::PathBuf;

use super::{CheckoutFailureKind, CheckoutRuntime, Fixture, LocalCheckout, git, git_output};

#[test]
fn reset_workspace_uses_origin_default_branch_and_restores_initial_branch() {
    for default_branch in ["main", "master"] {
        reset_workspace_from_default_branch(default_branch, "renamed-workspace", false);
    }
}

#[test]
fn reset_workspace_checks_out_existing_initial_branch_and_updates_it_to_latest_default() {
    for default_branch in ["main", "master"] {
        reset_workspace_from_default_branch(default_branch, "renamed-workspace", true);
    }
}

#[test]
fn reset_workspace_updates_the_already_checked_out_initial_branch() {
    reset_workspace_from_default_branch("main", "initial-workspace", true);
}

#[test]
fn reset_workspace_removes_untracked_files_directories_and_nested_repositories() {
    let fixture = ResetFixture::new("main", "initial-workspace", true);
    let scratch = fixture.linked.join("scratch");
    std::fs::create_dir_all(scratch.join("nested")).expect("create untracked directories");
    std::fs::write(scratch.join("nested/file.txt"), "discard\n")
        .expect("write nested untracked file");
    let nested_repository = fixture.linked.join("nested-repository");
    git(
        &fixture.linked,
        &[
            "init",
            "-b",
            "main",
            nested_repository.to_str().expect("UTF-8 nested repo path"),
        ],
    );
    std::fs::write(nested_repository.join("note.txt"), "discard\n")
        .expect("write nested repository file");
    std::fs::write(
        fixture.repository.repo.join(".git/info/exclude"),
        "ignored-cache/\n",
    )
    .expect("configure ignored cache directory");
    let ignored_cache = fixture.linked.join("ignored-cache");
    std::fs::create_dir(&ignored_cache).expect("create ignored cache directory");
    std::fs::write(ignored_cache.join("cache.txt"), "keep\n").expect("write ignored cache file");
    let outside = fixture.repository.repo.join("outside-reset.txt");
    std::fs::write(&outside, "keep\n").expect("write file outside the reset worktree");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, fixture.linked.join("untracked-link"))
        .expect("create untracked symlink");

    fixture
        .runtime
        .reset_workspace(
            fixture.linked.to_str().expect("UTF-8 worktree path"),
            "initial-workspace",
        )
        .expect("reset managed workspace");

    fixture.assert_reset();
    assert!(!scratch.exists(), "untracked directories must be removed");
    assert!(
        !nested_repository.exists(),
        "untracked repositories must be removed"
    );
    #[cfg(unix)]
    assert!(!fixture.linked.join("untracked-link").exists());
    assert_eq!(
        std::fs::read_to_string(ignored_cache.join("cache.txt")).expect("read ignored cache"),
        "keep\n"
    );
    assert_eq!(
        std::fs::read_to_string(outside).expect("read outside file"),
        "keep\n"
    );
    assert!(
        git_output(
            &fixture.linked,
            &["status", "--porcelain", "--untracked-files=all"]
        )
        .is_empty()
    );
}

fn reset_workspace_from_default_branch(
    default_branch: &str,
    workspace_branch: &str,
    initial_branch_exists: bool,
) {
    let fixture = ResetFixture::new(default_branch, workspace_branch, initial_branch_exists);
    for branch in [workspace_branch, "initial-workspace"] {
        for (setting, value) in [
            ("remote", "origin"),
            ("merge", "refs/heads/old-pr"),
            ("pushRemote", "paseo-pr-42"),
            ("gh-merge-base", "old-base"),
        ] {
            git(
                &fixture.linked,
                &["config", &format!("branch.{branch}.{setting}"), value],
            );
        }
    }
    fixture
        .runtime
        .reset_workspace(fixture.linked.to_str().unwrap(), "initial-workspace")
        .unwrap();

    fixture.assert_reset();
    let reset_at = git_output(
        &fixture.linked,
        &[
            "config",
            "--local",
            "--get",
            "branch.initial-workspace.ait-reset-at",
        ],
    );
    let reset_at = chrono::DateTime::parse_from_rfc3339(&reset_at).unwrap();
    assert!((chrono::Utc::now() - reset_at.with_timezone(&chrono::Utc)).num_seconds() < 30);
    for setting in ["remote", "merge", "pushRemote", "gh-merge-base"] {
        let output = std::process::Command::new("git")
            .args([
                "config",
                "--local",
                "--get-all",
                &format!("branch.initial-workspace.{setting}"),
            ])
            .current_dir(&fixture.linked)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{setting}");
        assert!(output.stdout.is_empty(), "{setting}");
    }
    if initial_branch_exists && workspace_branch != "initial-workspace" {
        assert_eq!(
            git_output(&fixture.linked, &["rev-parse", workspace_branch]),
            fixture.local_commit
        );
    }
    assert!(
        git_output(
            &fixture.remote,
            &["for-each-ref", "refs/heads/initial-workspace"]
        )
        .is_empty()
    );
}

struct ResetFixture {
    repository: Fixture,
    remote: PathBuf,
    linked: PathBuf,
    runtime: LocalCheckout,
    latest: String,
    local_commit: String,
    remote_ref: String,
}

impl ResetFixture {
    fn new(default_branch: &str, workspace_branch: &str, initial_branch_exists: bool) -> Self {
        let fixture = Fixture::new();
        let remote = fixture.temp.path().join("remote.git");
        git(
            fixture.temp.path(),
            &[
                "init",
                "--bare",
                "-b",
                default_branch,
                remote.to_str().unwrap(),
            ],
        );
        git(
            &fixture.repo,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(
            &fixture.repo,
            &["push", "-u", "origin", &format!("main:{default_branch}")],
        );

        let managed = fixture.temp.path().join("managed");
        let linked = managed.join("repository-hash").join("initial-workspace");
        std::fs::create_dir_all(linked.parent().unwrap()).unwrap();
        git(
            &fixture.repo,
            &[
                "worktree",
                "add",
                "-b",
                "initial-workspace",
                linked.to_str().unwrap(),
            ],
        );
        if workspace_branch != "initial-workspace" {
            git(&linked, &["branch", "-m", workspace_branch]);
            if initial_branch_exists {
                git(&fixture.repo, &["branch", "initial-workspace", "main"]);
            }
        }
        std::fs::write(linked.join("tracked.txt"), "local commit\n").unwrap();
        git(&linked, &["add", "tracked.txt"]);
        git(&linked, &["commit", "-m", "local work"]);
        let local_commit = git_output(&linked, &["rev-parse", "HEAD"]);
        std::fs::write(linked.join("tracked.txt"), "dirty changes\n").unwrap();
        std::fs::write(linked.join("untracked.txt"), "keep\n").unwrap();

        let writer = fixture.temp.path().join("writer");
        git(
            fixture.temp.path(),
            &["clone", remote.to_str().unwrap(), writer.to_str().unwrap()],
        );
        git(&writer, &["config", "user.name", "Server Test"]);
        git(&writer, &["config", "user.email", "server@example.invalid"]);
        std::fs::write(writer.join("tracked.txt"), "latest default\n").unwrap();
        git(&writer, &["add", "tracked.txt"]);
        git(&writer, &["commit", "-m", "new default"]);
        git(&writer, &["push", "origin", default_branch]);
        let latest = git_output(&writer, &["rev-parse", "HEAD"]);
        let remote_ref = format!("origin/{default_branch}");
        assert_ne!(git_output(&linked, &["rev-parse", &remote_ref]), latest);

        let runtime = LocalCheckout::new(managed);
        Self {
            repository: fixture,
            remote,
            linked,
            runtime,
            latest,
            local_commit,
            remote_ref,
        }
    }

    fn assert_reset(&self) {
        assert_eq!(
            git_output(&self.linked, &["branch", "--show-current"]),
            "initial-workspace"
        );
        assert_eq!(
            git_output(&self.linked, &["rev-parse", "HEAD"]),
            self.latest
        );
        assert_eq!(
            git_output(&self.linked, &["rev-parse", "refs/heads/initial-workspace"]),
            self.latest
        );
        assert_eq!(
            git_output(&self.linked, &["rev-parse", &self.remote_ref]),
            self.latest
        );
        assert_eq!(
            std::fs::read_to_string(self.linked.join("tracked.txt")).unwrap(),
            "latest default\n"
        );
        assert!(
            !self.linked.join("untracked.txt").exists(),
            "reset must remove untracked files"
        );
    }
}

#[test]
fn reset_workspace_force_pushes_the_live_same_named_origin_branch() {
    for default_branch in ["main", "master"] {
        for (workspace_branch, initial_branch_exists) in [
            ("renamed-workspace", false),
            ("renamed-workspace", true),
            ("initial-workspace", true),
        ] {
            let fixture =
                ResetFixture::new(default_branch, workspace_branch, initial_branch_exists);
            git(
                &fixture.linked,
                &["push", "origin", "HEAD:refs/heads/initial-workspace"],
            );
            // A stale tracking ref must not supply the force-push lease.
            let stale_tip = git_output(&fixture.linked, &["rev-parse", &fixture.remote_ref]);
            git(
                &fixture.linked,
                &[
                    "update-ref",
                    "refs/remotes/origin/initial-workspace",
                    &stale_tip,
                ],
            );
            git(
                &fixture.linked,
                &[
                    "branch",
                    "--set-upstream-to",
                    &fixture.remote_ref,
                    workspace_branch,
                ],
            );
            let main_head = git_output(&fixture.repository.repo, &["rev-parse", "HEAD"]);

            fixture
                .runtime
                .reset_workspace(fixture.linked.to_str().unwrap(), "initial-workspace")
                .unwrap();

            fixture.assert_reset();
            assert_eq!(
                git_output(&fixture.repository.repo, &["rev-parse", "HEAD"]),
                main_head
            );
            assert_eq!(
                git_output(
                    &fixture.remote,
                    &["rev-parse", "refs/heads/initial-workspace"]
                ),
                fixture.latest
            );
            assert_eq!(
                git_output(
                    &fixture.remote,
                    &["rev-parse", &format!("refs/heads/{default_branch}")]
                ),
                fixture.latest
            );
        }
    }
}

#[test]
fn reset_workspace_pushes_a_remote_branch_without_a_local_tracking_ref() {
    let fixture = ResetFixture::new("main", "renamed-workspace", false);
    git(
        &fixture.linked,
        &["push", "origin", "HEAD:refs/heads/initial-workspace"],
    );
    git(
        &fixture.linked,
        &["update-ref", "-d", "refs/remotes/origin/initial-workspace"],
    );

    fixture
        .runtime
        .reset_workspace(fixture.linked.to_str().unwrap(), "initial-workspace")
        .unwrap();

    fixture.assert_reset();
    assert_eq!(
        git_output(
            &fixture.remote,
            &["rev-parse", "refs/heads/initial-workspace"]
        ),
        fixture.latest
    );
}

#[test]
fn reset_workspace_does_not_recreate_a_deleted_remote_branch_from_a_stale_tracking_ref() {
    let fixture = ResetFixture::new("main", "renamed-workspace", false);
    git(
        &fixture.linked,
        &["push", "origin", "HEAD:refs/heads/initial-workspace"],
    );
    git(
        &fixture.remote,
        &["update-ref", "-d", "refs/heads/initial-workspace"],
    );
    assert_eq!(
        git_output(
            &fixture.linked,
            &["rev-parse", "refs/remotes/origin/initial-workspace"]
        ),
        fixture.local_commit
    );

    fixture
        .runtime
        .reset_workspace(fixture.linked.to_str().unwrap(), "initial-workspace")
        .unwrap();

    fixture.assert_reset();
    assert!(
        git_output(
            &fixture.remote,
            &["for-each-ref", "refs/heads/initial-workspace"]
        )
        .is_empty()
    );
}

#[test]
fn reset_workspace_reports_a_rejected_force_push_and_can_retry() {
    let fixture = ResetFixture::new("main", "renamed-workspace", false);
    git(
        &fixture.linked,
        &["push", "origin", "HEAD:refs/heads/initial-workspace"],
    );
    git(
        &fixture.remote,
        &["config", "receive.denyNonFastForwards", "true"],
    );

    let error = fixture
        .runtime
        .reset_workspace(fixture.linked.to_str().unwrap(), "initial-workspace")
        .unwrap_err();

    assert_eq!(error.kind, CheckoutFailureKind::Unknown);
    assert!(error.message.contains("Local workspace was reset"));
    assert!(
        error
            .message
            .contains("resetting origin/initial-workspace failed")
    );
    fixture.assert_reset();
    assert_eq!(
        git_output(
            &fixture.remote,
            &["rev-parse", "refs/heads/initial-workspace"]
        ),
        fixture.local_commit
    );

    git(
        &fixture.remote,
        &["config", "receive.denyNonFastForwards", "false"],
    );
    fixture
        .runtime
        .reset_workspace(fixture.linked.to_str().unwrap(), "initial-workspace")
        .unwrap();
    assert_eq!(
        git_output(
            &fixture.remote,
            &["rev-parse", "refs/heads/initial-workspace"]
        ),
        fixture.latest
    );
}

#[cfg(unix)]
#[test]
fn reset_workspace_uses_one_push_for_only_the_initial_branch() {
    let fixture = ResetFixture::new("main", "renamed-workspace", false);
    git(
        &fixture.linked,
        &[
            "push",
            "origin",
            "HEAD:refs/heads/initial-workspace",
            "HEAD:refs/heads/renamed-workspace",
        ],
    );
    git(&fixture.linked, &["fetch", "origin"]);
    git(&fixture.linked, &["config", "push.followTags", "true"]);
    git(
        &fixture.linked,
        &["tag", "-a", "local-tag", &fixture.latest, "-m", "local tag"],
    );
    install_hook(
        &fixture.remote.join("hooks"),
        "pre-receive",
        "#!/bin/sh\ncat >> push-attempts\n",
    );

    fixture
        .runtime
        .reset_workspace(fixture.linked.to_str().unwrap(), "initial-workspace")
        .unwrap();

    fixture.assert_reset();
    assert_eq!(
        std::fs::read_to_string(fixture.remote.join("push-attempts")).unwrap(),
        format!(
            "{} {} refs/heads/initial-workspace\n",
            fixture.local_commit, fixture.latest
        )
    );
    assert_eq!(
        git_output(
            &fixture.remote,
            &["rev-parse", "refs/heads/renamed-workspace"]
        ),
        fixture.local_commit
    );
    assert!(git_output(&fixture.remote, &["for-each-ref", "refs/tags"]).is_empty());
}

#[cfg(unix)]
#[test]
fn reset_workspace_rejects_remote_updates_or_deletion_after_preflight() {
    for deleted in [false, true] {
        let fixture = ResetFixture::new("main", "renamed-workspace", true);
        git(
            &fixture.linked,
            &["push", "origin", "HEAD:refs/heads/initial-workspace"],
        );
        let concurrent = git_output(
            &fixture.remote,
            &[
                "-c",
                "user.name=Server Test",
                "-c",
                "user.email=server@example.invalid",
                "commit-tree",
                &format!("{}^{{tree}}", fixture.latest),
                "-p",
                &fixture.latest,
                "-m",
                "concurrent update",
            ],
        );
        let update_arguments = if deleted {
            "-d refs/heads/initial-workspace".to_owned()
        } else {
            format!("refs/heads/initial-workspace {concurrent}")
        };
        let script = format!(
            "#!/bin/sh\ngit --git-dir='{}' update-ref {update_arguments}\n",
            fixture.remote.display()
        );
        install_hook(
            &fixture.repository.repo.join(".git/hooks"),
            "post-checkout",
            &script,
        );

        let error = fixture
            .runtime
            .reset_workspace(fixture.linked.to_str().unwrap(), "initial-workspace")
            .unwrap_err();

        assert!(
            error.message.contains("Local workspace was reset"),
            "{error:?}"
        );
        assert!(error.message.contains("stale info"), "{error:?}");
        fixture.assert_reset();
        if deleted {
            assert!(
                git_output(
                    &fixture.remote,
                    &["for-each-ref", "refs/heads/initial-workspace"]
                )
                .is_empty()
            );
        } else {
            assert_eq!(
                git_output(
                    &fixture.remote,
                    &["rev-parse", "refs/heads/initial-workspace"]
                ),
                concurrent
            );
        }
    }
}

#[cfg(unix)]
fn install_hook(hooks: &std::path::Path, name: &str, script: &str) {
    use std::os::unix::fs::PermissionsExt;

    std::fs::create_dir_all(hooks).unwrap();
    let hook = hooks.join(name);
    std::fs::write(&hook, script).unwrap();
    std::fs::set_permissions(hook, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn reset_workspace_preserves_both_worktrees_when_initial_branch_is_checked_out_elsewhere() {
    let fixture = Fixture::new();
    let remote = fixture.temp.path().join("remote.git");
    git(
        fixture.temp.path(),
        &["init", "--bare", "-b", "main", remote.to_str().unwrap()],
    );
    git(
        &fixture.repo,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&fixture.repo, &["push", "-u", "origin", "main"]);
    let managed = fixture.temp.path().join("managed");
    let linked = managed.join("repository-hash").join("initial-workspace");
    std::fs::create_dir_all(linked.parent().unwrap()).unwrap();
    git(
        &fixture.repo,
        &[
            "worktree",
            "add",
            "-b",
            "renamed-workspace",
            linked.to_str().unwrap(),
        ],
    );
    let other = fixture.temp.path().join("other-worktree");
    git(
        &fixture.repo,
        &[
            "worktree",
            "add",
            "-b",
            "initial-workspace",
            other.to_str().unwrap(),
        ],
    );
    std::fs::write(linked.join("tracked.txt"), "local work\n").unwrap();
    git(&linked, &["commit", "-am", "local work"]);
    std::fs::write(linked.join("tracked.txt"), "dirty changes\n").unwrap();
    std::fs::write(other.join("tracked.txt"), "other dirty changes\n").unwrap();
    let before = git_output(&linked, &["rev-parse", "HEAD"]);
    let other_before = git_output(&other, &["rev-parse", "HEAD"]);

    let runtime = LocalCheckout::new(managed);
    assert!(
        runtime
            .reset_workspace(linked.to_str().unwrap(), "initial-workspace")
            .is_err()
    );

    assert_eq!(
        git_output(&linked, &["branch", "--show-current"]),
        "renamed-workspace"
    );
    assert_eq!(git_output(&linked, &["rev-parse", "HEAD"]), before);
    assert_eq!(git_output(&other, &["rev-parse", "HEAD"]), other_before);
    assert_eq!(
        std::fs::read_to_string(linked.join("tracked.txt")).unwrap(),
        "dirty changes\n"
    );
    assert_eq!(
        std::fs::read_to_string(other.join("tracked.txt")).unwrap(),
        "other dirty changes\n"
    );
}

#[test]
fn reset_workspace_rejects_unmanaged_checkout_without_moving_head() {
    let fixture = Fixture::new();
    let untracked = fixture.repo.join("untracked.txt");
    std::fs::write(&untracked, "keep\n").expect("write unmanaged untracked file");
    let before = git_output(&fixture.repo, &["rev-parse", "HEAD"]);
    let runtime = LocalCheckout::new(fixture.temp.path().join("managed"));

    let error = runtime
        .reset_workspace(fixture.repo.to_str().unwrap(), "initial-workspace")
        .unwrap_err();

    assert_eq!(error.kind, CheckoutFailureKind::NotAllowed);
    assert_eq!(git_output(&fixture.repo, &["rev-parse", "HEAD"]), before);
    assert_eq!(
        std::fs::read_to_string(untracked).expect("read unmanaged file"),
        "keep\n"
    );
}

#[test]
fn reset_workspace_preserves_branch_and_head_when_remote_lookup_fails() {
    let fixture = Fixture::new();
    let managed = fixture.temp.path().join("managed");
    let linked = managed.join("repository-hash").join("initial-workspace");
    std::fs::create_dir_all(linked.parent().unwrap()).unwrap();
    git(
        &fixture.repo,
        &[
            "worktree",
            "add",
            "-b",
            "initial-workspace",
            linked.to_str().unwrap(),
        ],
    );
    git(&linked, &["branch", "-m", "renamed-workspace"]);
    git(&fixture.repo, &["branch", "initial-workspace", "main"]);
    git(
        &fixture.repo,
        &["remote", "add", "origin", "/missing/repository.git"],
    );
    let before = git_output(&linked, &["rev-parse", "HEAD"]);
    let untracked = linked.join("untracked.txt");
    std::fs::write(&untracked, "keep\n").expect("write file before remote lookup failure");

    let runtime = LocalCheckout::new(managed);
    assert!(
        runtime
            .reset_workspace(linked.to_str().unwrap(), "initial-workspace")
            .is_err()
    );

    assert_eq!(
        git_output(&linked, &["branch", "--show-current"]),
        "renamed-workspace"
    );
    assert_eq!(git_output(&linked, &["rev-parse", "HEAD"]), before);
    assert_eq!(
        std::fs::read_to_string(untracked).expect("read file after failed reset"),
        "keep\n"
    );
}
