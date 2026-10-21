use super::*;

#[test]
fn missing_upstream_does_not_expose_the_unresolved_revision() {
    let fixture = Fixture::new();
    git(
        &fixture.repo,
        &[
            "remote",
            "add",
            "origin",
            "https://example.invalid/repo.git",
        ],
    );
    git(&fixture.repo, &["config", "branch.main.remote", "origin"]);
    git(
        &fixture.repo,
        &["config", "branch.main.merge", "refs/heads/main"],
    );
    let runtime = LocalCheckout::new(fixture.temp.path().join("managed"));

    let status = runtime.status(fixture.repo.to_str().unwrap()).unwrap();

    assert_eq!(status.current_branch.as_deref(), Some("main"));
    assert_eq!(status.upstream_ref, None);
    assert_eq!(status.ahead_of_origin, None);
    assert_eq!(status.behind_of_origin, None);
    assert_eq!(
        super::super::git_optional(
            &fixture.repo,
            &["rev-parse", "--symbolic-full-name", "@{upstream}"]
        )
        .unwrap(),
        None
    );
}

#[test]
fn pruned_upstream_clears_the_reference_and_counts() {
    let fixture = Fixture::new();
    git(
        &fixture.repo,
        &[
            "remote",
            "add",
            "origin",
            "https://example.invalid/repo.git",
        ],
    );
    git(
        &fixture.repo,
        &["update-ref", "refs/remotes/origin/main", "HEAD"],
    );
    git(&fixture.repo, &["branch", "--set-upstream-to=origin/main"]);
    let runtime = LocalCheckout::new(fixture.temp.path().join("managed"));
    let cwd = fixture.repo.to_str().unwrap();
    let before = runtime.status(cwd).unwrap();
    assert_eq!(
        before.upstream_ref.as_deref(),
        Some("refs/remotes/origin/main")
    );
    git(
        &fixture.repo,
        &["update-ref", "-d", "refs/remotes/origin/main"],
    );

    let status = runtime.status(cwd).unwrap();

    assert_eq!(status.current_branch.as_deref(), Some("main"));
    assert_eq!(status.upstream_ref, None);
    assert_eq!(status.ahead_of_origin, None);
    assert_eq!(status.behind_of_origin, None);
}

#[test]
fn upstream_preserves_the_remote_and_branch_name_with_divergence() {
    let fixture = Fixture::new();
    git(
        &fixture.repo,
        &[
            "remote",
            "add",
            "upstream",
            "https://example.invalid/repo.git",
        ],
    );
    git(
        &fixture.repo,
        &["update-ref", "refs/remotes/upstream/trunk", "HEAD"],
    );
    git(
        &fixture.repo,
        &["branch", "--set-upstream-to=upstream/trunk"],
    );
    git(
        &fixture.repo,
        &["commit", "--allow-empty", "-m", "local change"],
    );
    let runtime = LocalCheckout::new(fixture.temp.path().join("managed"));

    let status = runtime.status(fixture.repo.to_str().unwrap()).unwrap();

    assert_eq!(
        status.upstream_ref.as_deref(),
        Some("refs/remotes/upstream/trunk")
    );
    assert_eq!(status.ahead_of_origin, Some(1));
    assert_eq!(status.behind_of_origin, Some(0));
}

#[test]
fn local_upstream_is_a_fully_qualified_local_reference() {
    let fixture = Fixture::new();
    git(
        &fixture.repo,
        &["checkout", "-b", "feature", "--track", "main"],
    );
    let runtime = LocalCheckout::new(fixture.temp.path().join("managed"));

    let status = runtime.status(fixture.repo.to_str().unwrap()).unwrap();

    assert_eq!(status.current_branch.as_deref(), Some("feature"));
    assert_eq!(status.upstream_ref.as_deref(), Some("refs/heads/main"));
    assert_eq!(status.ahead_of_origin, Some(0));
    assert_eq!(status.behind_of_origin, Some(0));
}

#[test]
fn unconfigured_upstream_and_detached_head_have_no_upstream() {
    let fixture = Fixture::new();
    let runtime = LocalCheckout::new(fixture.temp.path().join("managed"));
    let cwd = fixture.repo.to_str().unwrap();
    let status = runtime.status(cwd).unwrap();
    assert_eq!(status.current_branch.as_deref(), Some("main"));
    assert_eq!(status.upstream_ref, None);
    git(&fixture.repo, &["checkout", "--detach"]);

    let detached = runtime.status(cwd).unwrap();

    assert_eq!(detached.current_branch, None);
    assert_eq!(detached.upstream_ref, None);
    assert_eq!(detached.ahead_of_origin, None);
    assert_eq!(detached.behind_of_origin, None);
}
