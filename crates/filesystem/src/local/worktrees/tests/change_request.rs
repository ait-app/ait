//! Offline ports of Paseo's PR ref fallback, branch collision and fork tracking scenarios.

use std::os::unix::fs::PermissionsExt;

use super::*;
use metadata::ports::worktrees::WorktreeChangeRequest;

const GH_FIXTURE: &str = include_str!("../../../../tests/fixtures/gh_checkout.py");

mod gitlab;

fn fixture(cross: bool) -> (Fixture, LocalManagedWorktrees, PathBuf) {
    let fixture = Fixture::new();
    run(&fixture.repository, &["checkout", "-b", "topic"]);
    std::fs::write(fixture.repository.join("change.txt"), "PR content").unwrap();
    paseo::commit(&fixture.repository, "PR head");
    let remote = fixture.root.path().join("remote.git");
    run(
        fixture.root.path(),
        &[
            "clone",
            "--bare",
            &path(&fixture.repository),
            &path(&remote),
        ],
    );
    run(
        &remote,
        &["update-ref", "refs/pull/123/head", "refs/heads/topic"],
    );
    run(&fixture.repository, &["checkout", "main"]);
    run(
        &fixture.repository,
        &["remote", "add", "origin", &path(&remote)],
    );
    let executable = fixture.root.path().join("gh");
    std::fs::write(&executable, GH_FIXTURE).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(fixture.root.path().join("pull-request.json"), serde_json::json!({
        "data":{"repository":{"pullRequest":{"number":123,"headRefName":"topic","baseRefName":"main",
            "isCrossRepository":cross,"headRepositoryOwner":{"login":"ForkOwner"},
            "headRepository":{"sshUrl":"git@github.com:ForkOwner/repository.git", "url":"https://github.com/ForkOwner/repository"}
        }}}
    }).to_string()).unwrap();
    let mut adapter = fixture.adapter();
    adapter.forge = crate::local::forge::LocalForge::with_executable(executable);
    (fixture, adapter, remote)
}

fn target(
    adapter: &LocalManagedWorktrees,
    fixture: &Fixture,
) -> crate::ports::worktrees::ChangeRequestCheckout {
    adapter
        .resolve_change_request(
            &path(&fixture.repository),
            &WorktreeChangeRequest {
                forge: Some("github".into()),
                number: 123,
                project_path: None,
            },
            None,
        )
        .unwrap()
}

fn create(adapter: &LocalManagedWorktrees, fixture: &Fixture) -> CreatedManagedWorktree {
    let target = target(adapter, fixture);
    adapter
        .create(&ManagedWorktreeCreate {
            cwd: path(&fixture.repository),
            slug: "pull-request".into(),
            mode: WorktreeCreateMode::ChangeRequest(target),
        })
        .unwrap()
}

fn read_git(cwd: &Path, args: &[&str]) -> String {
    String::from_utf8(
        Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_owned()
}

#[test]
fn same_repo_pr_uses_unique_local_branches_and_tracks_the_original_remote_head() {
    let (fixture, adapter, remote) = fixture(false);
    let original = read_git(&fixture.repository, &["rev-parse", "topic"]);
    let first = create(&adapter, &fixture);
    let second = create(&adapter, &fixture);
    assert_eq!(first.branch_name, "topic-1");
    assert_eq!(second.branch_name, "topic-2");
    assert_eq!(first.comparison_base_ref.as_deref(), Some("main"));
    let cwd = Path::new(&first.worktree_path);
    assert_eq!(
        read_git(cwd, &["rev-parse", "HEAD"]),
        read_git(&remote, &["rev-parse", "refs/pull/123/head"])
    );
    assert_eq!(
        read_git(&fixture.repository, &["rev-parse", "topic"]),
        original
    );
    assert_eq!(
        read_git(cwd, &["rev-parse", "--abbrev-ref", "@{upstream}"]),
        "origin/topic"
    );
    assert_eq!(
        read_git(cwd, &["config", "branch.topic-1.pushRemote"]),
        "paseo-pr-123"
    );
    assert_eq!(
        read_git(cwd, &["config", "remote.paseo-pr-123.push"]),
        "HEAD:refs/heads/topic"
    );
    assert!(read_git(cwd, &["for-each-ref", "refs/ait/checkout"]).is_empty());
}

#[test]
fn fork_pr_falls_back_to_upstream_and_tracks_the_fork_with_a_prefixed_branch() {
    let (fixture, adapter, remote) = fixture(true);
    run(
        &fixture.repository,
        &["remote", "rename", "origin", "upstream"],
    );
    let missing = fixture.root.path().join("missing.git");
    run(
        &fixture.repository,
        &["remote", "add", "origin", &path(&missing)],
    );
    run(
        &fixture.repository,
        &[
            "config",
            &format!("url.{}.insteadOf", remote.display()),
            "git@github.com:ForkOwner/repository.git",
        ],
    );
    let target = target(&adapter, &fixture);
    assert_eq!(
        target.untrusted_repository.as_deref(),
        Some("ForkOwner/repository")
    );
    let created = create(&adapter, &fixture);
    assert_eq!(created.branch_name, "forkowner/topic");
    assert_eq!(created.comparison_base_ref.as_deref(), Some("main"));
    let cwd = Path::new(&created.worktree_path);
    assert_eq!(
        read_git(cwd, &["rev-parse", "--abbrev-ref", "@{upstream}"]),
        "paseo-pr-123/topic"
    );
    assert_eq!(
        read_git(cwd, &["config", "remote.paseo-pr-123.url"]),
        "git@github.com:ForkOwner/repository.git"
    );
    assert_eq!(
        read_git(cwd, &["config", "remote.paseo-pr-123.push"]),
        "HEAD:refs/heads/topic"
    );
}

#[test]
fn invalid_and_missing_pull_requests_do_not_create_a_worktree() {
    let (fixture, adapter, _) = fixture(false);
    for (forge, number) in [
        ("gitlab", 123),
        ("github", 0),
        ("github", 9_007_199_254_740_992),
    ] {
        assert!(
            adapter
                .resolve_change_request(
                    &path(&fixture.repository),
                    &WorktreeChangeRequest {
                        forge: Some(forge.into()),
                        number,
                        project_path: None,
                    },
                    None
                )
                .is_err()
        );
    }
    std::fs::write(
        fixture.root.path().join("pull-request.json"),
        "{\"data\":{\"repository\":{\"pullRequest\":null}}}",
    )
    .unwrap();
    assert!(
        adapter
            .resolve_change_request(
                &path(&fixture.repository),
                &WorktreeChangeRequest {
                    forge: None,
                    number: 123,
                    project_path: None,
                },
                None
            )
            .is_err()
    );
    assert!(adapter.list(&path(&fixture.repository)).unwrap().is_empty());
    assert_eq!(branch(&fixture.repository), "main");
}

#[test]
fn absent_pull_refs_leave_existing_branches_and_temporary_refs_unchanged() {
    let (fixture, adapter, remote) = fixture(false);
    run(&remote, &["update-ref", "-d", "refs/pull/123/head"]);
    let target = target(&adapter, &fixture);
    let original = read_git(&fixture.repository, &["rev-parse", "topic"]);
    assert!(
        adapter
            .create(&ManagedWorktreeCreate {
                cwd: path(&fixture.repository),
                slug: "missing-pull".into(),
                mode: WorktreeCreateMode::ChangeRequest(target),
            })
            .is_err()
    );
    assert_eq!(
        read_git(&fixture.repository, &["rev-parse", "topic"]),
        original
    );
    assert!(read_git(&fixture.repository, &["for-each-ref", "refs/ait/checkout"]).is_empty());
    assert!(adapter.list(&path(&fixture.repository)).unwrap().is_empty());
}
