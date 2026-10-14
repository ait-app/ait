use super::*;
use model::workspace::naming::WorkspaceBranchNamer;

#[test]
fn placeholder_rename_checks_ownership_current_branch_upstream_and_collisions() {
    let fixture = Fixture::new();
    let managed = fixture.temp.path().join("worktrees");
    let linked = managed.join("hash").join("placeholder");
    std::fs::create_dir_all(linked.parent().unwrap()).unwrap();
    git(
        &fixture.repo,
        &[
            "worktree",
            "add",
            "-b",
            "placeholder",
            linked.to_str().unwrap(),
        ],
    );
    git(&fixture.repo, &["branch", "fix/title"]);
    let runtime = LocalCheckout::new(managed);
    assert_eq!(
        WorkspaceBranchNamer::rename(
            &runtime,
            fixture.repo.to_str().unwrap(),
            "main",
            "fix/unsafe"
        ),
        None
    );
    assert_eq!(
        WorkspaceBranchNamer::rename(&runtime, linked.to_str().unwrap(), "wrong", "fix/unsafe"),
        None
    );
    assert_eq!(
        WorkspaceBranchNamer::rename(
            &runtime,
            linked.to_str().unwrap(),
            "placeholder",
            "fix/title"
        ),
        Some("fix/title-2".into())
    );
    assert_eq!(
        git_output(&linked, &["branch", "--show-current"]),
        "fix/title-2"
    );
    git(&linked, &["branch", "--set-upstream-to=main"]);
    assert_eq!(
        WorkspaceBranchNamer::rename(
            &runtime,
            linked.to_str().unwrap(),
            "fix/title-2",
            "fix/unsafe"
        ),
        None
    );
}
