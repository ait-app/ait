use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use domain::workspace::records::PersistedWorkspaceRecord;

use super::*;

#[cfg(target_os = "macos")]
mod enforcement;

fn confinement(root: &Path) -> WriteConfinement {
    let record: PersistedWorkspaceRecord = serde_json::from_value(serde_json::json!({
        "workspaceId": "wks_0123456789abcdef",
        "projectId": "prj_1",
        "cwd": root,
        "kind": "worktree",
        "displayName": "feature",
        "worktreeRoot": root,
        "createdAt": "2026-01-01T00:00:00Z",
        "updatedAt": "2026-01-01T00:00:00Z",
        "archivedAt": null,
        "sandbox": "worktree_writes",
    }))
    .expect("valid workspace record");
    WriteConfinement::for_workspace(&record).expect("sandboxed workspace")
}

/// A main checkout at `repo` and a linked worktree at `wt`, as `git worktree add` lays out.
struct LinkedWorktree {
    _root: tempfile::TempDir,
    base: PathBuf,
}

impl LinkedWorktree {
    fn new(pointer: &str) -> Self {
        let root = tempfile::tempdir().expect("temporary directory");
        let base = root.path().to_path_buf();
        let private = base.join("repo/.git/worktrees/wt");
        fs::create_dir_all(&private).expect("private git dir");
        fs::write(private.join("commondir"), "../..\n").expect("commondir");
        fs::create_dir_all(base.join("wt")).expect("worktree");
        fs::write(base.join("wt/.git"), format!("gitdir: {pointer}\n")).expect("git pointer");
        Self { _root: root, base }
    }

    fn absolute() -> Self {
        let worktree = Self::new("placeholder");
        let pointer = worktree.base.join("repo/.git/worktrees/wt");
        fs::write(
            worktree.base.join("wt/.git"),
            format!("gitdir: {}\n", pointer.display()),
        )
        .expect("git pointer");
        worktree
    }

    fn real(&self, path: &str) -> PathBuf {
        self.base.join(path).canonicalize().expect("existing path")
    }
}

fn lookup(variables: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let variables: BTreeMap<String, String> = variables
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect();
    move |name| variables.get(name).cloned()
}

#[test]
fn unconfined_launch_runs_the_program_directly() {
    let command = command(Path::new("/bin/echo"), None, CODEX_STATE, &lookup(&[]))
        .expect("unconfined command");

    assert_eq!(command.as_std().get_program(), "/bin/echo");
    assert_eq!(command.as_std().get_args().count(), 0);
}

#[test]
fn policy_resolves_symlinks_and_includes_the_common_git_dir() {
    let worktree = LinkedWorktree::absolute();
    let home = tempfile::tempdir().expect("home");
    let link = worktree.base.join("link");
    std::os::unix::fs::symlink(worktree.base.join("wt"), &link).expect("symlink");
    let home_path = home.path().to_str().expect("utf-8 home");

    let policy = policy(&confinement(&link), &[], &lookup(&[("HOME", home_path)]))
        .expect("resolvable policy");

    let real_worktree = worktree.real("wt");
    let common = worktree.real("repo/.git");
    assert_ne!(real_worktree, link, "the symlink is resolved");
    assert_eq!(policy.trees(), [real_worktree.clone(), common.clone()]);
    assert!(policy.protected().contains(&real_worktree.join(".git")));
    assert!(policy.protected().contains(&common.join("config")));
    assert!(policy.protected().contains(&common.join("hooks")));
}

#[test]
fn relative_git_pointer_is_resolved_against_the_worktree() {
    let worktree = LinkedWorktree::new("../repo/.git/worktrees/wt");
    let home = tempfile::tempdir().expect("home");

    let policy = policy(
        &confinement(&worktree.base.join("wt")),
        &[],
        &lookup(&[("HOME", home.path().to_str().expect("utf-8 home"))]),
    )
    .expect("resolvable policy");

    assert!(policy.trees().contains(&worktree.real("repo/.git")));
}

#[test]
fn main_checkout_uses_its_git_directory_for_both_roles() {
    let root = tempfile::tempdir().expect("checkout");
    fs::create_dir(root.path().join(".git")).expect("git dir");

    let policy = policy(
        &confinement(root.path()),
        &[],
        &lookup(&[("HOME", "/nonexistent-home")]),
    )
    .expect("resolvable policy");

    let real = root.path().canonicalize().expect("real checkout");
    assert_eq!(policy.trees(), [real.clone(), real.join(".git")]);
}

#[test]
fn malformed_git_pointer_fails_closed() {
    let worktree = LinkedWorktree::new("");

    let error = policy(
        &confinement(&worktree.base.join("wt")),
        &[],
        &lookup(&[("HOME", "/home")]),
    )
    .expect_err("malformed pointer");

    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn state_locations_follow_overrides_defaults_prefixes_and_protections() {
    let worktree = LinkedWorktree::absolute();
    let home = tempfile::tempdir().expect("home");
    let override_dir = tempfile::tempdir().expect("override");
    let real_home = home.path().canonicalize().expect("real home");
    let real_override = override_dir.path().canonicalize().expect("real override");

    let policy = policy(
        &confinement(&worktree.base.join("wt")),
        CLAUDE_STATE,
        &lookup(&[
            ("HOME", home.path().to_str().expect("utf-8 home")),
            ("TMPDIR", override_dir.path().to_str().expect("utf-8 tmp")),
        ]),
    )
    .expect("resolvable policy");

    assert!(
        policy.trees().contains(&real_home.join(".claude")),
        "a missing state directory resolves through its existing parent"
    );
    assert!(
        policy
            .trees()
            .contains(&real_home.join("Library/Caches/claude-cli-nodejs"))
    );
    assert!(
        policy.trees().contains(&real_override),
        "TMPDIR is writable"
    );
    assert_eq!(policy.prefixes(), [real_home.join(".claude.json")]);
    assert!(
        policy
            .protected()
            .contains(&real_home.join(".claude/settings.json"))
    );

    let configured = policy_for_codex(&worktree, &real_home, &real_override);
    assert!(configured.trees().contains(&real_override));
    assert!(!configured.trees().contains(&real_home.join(".codex")));
    assert!(
        configured
            .protected()
            .contains(&real_override.join("config.toml"))
    );
}

fn policy_for_codex(worktree: &LinkedWorktree, home: &Path, codex_home: &Path) -> WritePolicy {
    policy(
        &confinement(&worktree.base.join("wt")),
        CODEX_STATE,
        &lookup(&[
            ("HOME", home.to_str().expect("utf-8 home")),
            ("CODEX_HOME", codex_home.to_str().expect("utf-8 codex home")),
        ]),
    )
    .expect("resolvable policy")
}

#[test]
fn xdg_base_variables_are_joined_with_the_provider_child() {
    let worktree = LinkedWorktree::absolute();
    let data = tempfile::tempdir().expect("xdg data");
    let real_data = data.path().canonicalize().expect("real data");

    let policy = policy(
        &confinement(&worktree.base.join("wt")),
        OPENCODE_STATE,
        &lookup(&[
            ("HOME", "/nonexistent-home"),
            ("XDG_DATA_HOME", data.path().to_str().expect("utf-8 data")),
        ]),
    )
    .expect("resolvable policy");

    assert!(policy.trees().contains(&real_data.join("opencode")));
    assert!(
        policy
            .trees()
            .contains(&PathBuf::from("/nonexistent-home/.config/opencode"))
    );
}

#[test]
fn missing_or_relative_home_fails_closed() {
    let worktree = LinkedWorktree::absolute();
    let root = worktree.base.join("wt");

    for variables in [&[][..], &[("HOME", "relative/home")][..]] {
        let error = policy(&confinement(&root), CODEX_STATE, &lookup(variables))
            .expect_err("home is required");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }
}

#[test]
fn unnormalized_state_paths_are_rejected() {
    let worktree = LinkedWorktree::absolute();

    let error = policy(
        &confinement(&worktree.base.join("wt")),
        CODEX_STATE,
        &lookup(&[("HOME", "/home"), ("CODEX_HOME", "/home/../etc")]),
    )
    .expect_err("parent components are rejected");

    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn unresolvable_worktree_fails_the_launch() {
    let missing = tempfile::tempdir().expect("parent").path().join("gone");

    let result = command(
        Path::new("/bin/sh"),
        Some(&confinement(&missing)),
        CODEX_STATE,
        &lookup(&[("HOME", "/home")]),
    );

    assert_eq!(result.err(), Some(AgentSessionError::Failed));
}
