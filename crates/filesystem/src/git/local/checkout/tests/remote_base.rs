use super::*;

#[test]
fn comparisons_prefer_origin_and_preserve_explicit_local_refs() {
    let fixture = Fixture::new();
    let initial = git_output(&fixture.repo, &["rev-parse", "HEAD"]);
    git(&fixture.repo, &["checkout", "-b", "feature"]);
    std::fs::write(fixture.repo.join("remote.txt"), "remote\n").unwrap();
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-m", "remote change"]);
    let remote = git_output(&fixture.repo, &["rev-parse", "HEAD"]);
    git(
        &fixture.repo,
        &["update-ref", "refs/remotes/origin/main", &remote],
    );
    git(&fixture.repo, &["reset", "--hard", &initial]);
    let runtime = LocalCheckout::new(fixture.temp.path().join("managed"));
    let status = runtime.status(fixture.repo.to_str().unwrap()).unwrap();
    assert_eq!(status.base_ref.as_deref(), Some("main"));
    assert_eq!(
        status.ahead_behind,
        Some(crate::git::ports::checkout::AheadBehind {
            ahead: 0,
            behind: 1
        })
    );
    assert_eq!(
        super::super::comparison_base(&fixture.repo, "main").unwrap(),
        "origin/main"
    );
    assert_eq!(
        super::super::comparison_base(&fixture.repo, "refs/heads/main").unwrap(),
        "refs/heads/main"
    );
    assert_eq!(
        super::super::comparison_base(&fixture.repo, "refs/remotes/origin/main").unwrap(),
        "refs/remotes/origin/main"
    );
}
