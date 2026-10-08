//! Actual GitLab MR ref fetching, branch fallback and fork tracking isolation.

use super::*;

fn gitlab_fixture(cross: bool) -> (Fixture, LocalManagedWorktrees, PathBuf) {
    let (fixture, adapter, remote) = fixture(false);
    let url = "https://gitlab.com/group/team/project.git";
    run(&fixture.repository, &["remote", "set-url", "origin", url]);
    run(
        &fixture.repository,
        &[
            "config",
            &format!("url.{}.insteadOf", remote.display()),
            url,
        ],
    );
    run(
        &remote,
        &[
            "update-ref",
            "refs/merge-requests/123/head",
            "refs/heads/topic",
        ],
    );
    let glab = fixture.root.path().join("glab");
    std::fs::write(&glab, "#!/bin/sh\ncat \"${0%/*}/merge-request.json\"\n").unwrap();
    std::fs::set_permissions(&glab, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(
        fixture.root.path().join("merge-request.json"),
        serde_json::json!({
            "iid":123, "source_branch":"topic", "target_branch":"main",
            "source_project_id": if cross { 2 } else { 1 }, "target_project_id":1
        })
        .to_string(),
    )
    .unwrap();
    (fixture, adapter, remote)
}

fn gitlab_target(adapter: &LocalManagedWorktrees, fixture: &Fixture) -> ChangeRequestCheckout {
    adapter
        .resolve_change_request(
            &path(&fixture.repository),
            &WorktreeChangeRequest {
                forge: Some("gitlab".to_owned()),
                number: 123,
                project_path: Some("group/team/project".to_owned()),
            },
            None,
        )
        .unwrap()
}

#[test]
fn gitlab_mr_ref_and_source_branch_fallback_create_trackable_unique_worktrees() {
    let (fixture, adapter, remote) = gitlab_fixture(false);
    let expected = read_git(&remote, &["rev-parse", "refs/heads/topic"]);
    for (slug, name) in [("mr-ref", "topic-1"), ("source-fallback", "topic-2")] {
        let created = adapter
            .create(&ManagedWorktreeCreate {
                cwd: path(&fixture.repository),
                slug: slug.to_owned(),
                mode: WorktreeCreateMode::ChangeRequest(gitlab_target(&adapter, &fixture)),
            })
            .unwrap();
        let cwd = Path::new(&created.worktree_path);
        assert_eq!(created.branch_name, name);
        assert_eq!(read_git(cwd, &["rev-parse", "HEAD"]), expected);
        assert_eq!(
            read_git(cwd, &["rev-parse", "--abbrev-ref", "@{upstream}"]),
            "origin/topic"
        );
        run(
            &remote,
            &["update-ref", "-d", "refs/merge-requests/123/head"],
        );
    }
    assert_eq!(branch(&fixture.repository), "main");
    assert!(read_git(&fixture.repository, &["for-each-ref", "refs/ait/checkout"]).is_empty());
}

#[test]
fn fork_mr_uses_the_target_mr_ref_without_tracking_the_target_branch() {
    let (fixture, adapter, remote) = gitlab_fixture(true);
    run(&remote, &["update-ref", "-d", "refs/heads/topic"]);
    let target = gitlab_target(&adapter, &fixture);
    assert!(target.untrusted_repository.is_some());
    assert!(!target.track_origin && target.push_remote_url.is_none());
    let created = adapter
        .create(&ManagedWorktreeCreate {
            cwd: path(&fixture.repository),
            slug: "fork-mr".to_owned(),
            mode: WorktreeCreateMode::ChangeRequest(target),
        })
        .unwrap();
    let cwd = Path::new(&created.worktree_path);
    assert_eq!(
        read_git(cwd, &["rev-parse", "HEAD"]),
        read_git(&remote, &["rev-parse", "refs/merge-requests/123/head"])
    );
    assert!(read_git(cwd, &["config", "branch.topic-1.remote"]).is_empty());
}
