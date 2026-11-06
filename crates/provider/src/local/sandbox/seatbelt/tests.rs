use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use super::*;

fn policy() -> WritePolicy {
    let mut policy = WritePolicy::worktree(
        Path::new("/Users/dev/wt"),
        Path::new("/Users/dev/repo/.git/worktrees/wt"),
        Path::new("/Users/dev/repo/.git"),
    );
    policy.allow_tree(PathBuf::from("/Users/dev/.codex"));
    policy.allow_prefix(PathBuf::from("/Users/dev/.claude.json"));
    policy.protect(PathBuf::from("/Users/dev/.codex/config.toml"));
    policy
}

#[test]
fn profile_denies_only_writes_and_lists_every_allowance() {
    let profile = profile(&policy()).expect("renderable policy");

    assert!(profile.starts_with("(version 1)\n(allow default)\n(deny file-write*)\n"));
    for rule in [
        "(subpath \"/private/tmp\")",
        "(subpath \"/private/var/folders\")",
        "(literal \"/dev/null\")",
        "(regex #\"^/dev/tty\")",
        "(regex #\"^/dev/fd/\")",
        "(subpath \"/Users/dev/wt\")",
        "(subpath \"/Users/dev/repo/.git\")",
        "(subpath \"/Users/dev/.codex\")",
        "(prefix \"/Users/dev/.claude.json\")",
    ] {
        assert!(profile.contains(rule), "missing {rule} in {profile}");
    }
    assert!(!profile.contains("(deny default)"));
}

#[test]
fn protected_paths_are_denied_after_the_allowances() {
    let profile = profile(&policy()).expect("renderable policy");

    let allowances = profile.find("(allow file-write*").expect("allow block");
    let protections = profile
        .rfind("(deny file-write*")
        .expect("protection block");
    assert!(protections > allowances);
    let protected = &profile[protections..];
    for path in [
        "/Users/dev/wt/.git",
        "/Users/dev/repo/.git/config",
        "/Users/dev/repo/.git/hooks",
        "/Users/dev/repo/.git/worktrees/wt/config.worktree",
        "/Users/dev/.codex/config.toml",
    ] {
        assert!(
            protected.contains(&format!("(subpath \"{path}\")")),
            "missing protection {path}"
        );
    }
}

#[test]
fn profile_without_protections_has_no_trailing_deny_block() {
    let mut policy = WritePolicy::default();
    policy.allow_tree(PathBuf::from("/Users/dev/wt"));

    let profile = profile(&policy).expect("renderable policy");

    assert_eq!(profile.matches("(deny file-write*").count(), 1);
}

#[test]
fn quotes_and_backslashes_are_escaped_inside_string_literals() {
    let mut policy = WritePolicy::default();
    policy.allow_tree(PathBuf::from(r#"/Users/dev/we"ird\dir"#));

    let profile = profile(&policy).expect("renderable policy");

    assert!(profile.contains(r#"(subpath "/Users/dev/we\"ird\\dir")"#));
}

#[test]
fn control_characters_are_rejected() {
    let mut policy = WritePolicy::default();
    policy.allow_tree(PathBuf::from(
        "/Users/dev/evil\")\n(allow file-write* (subpath \"/",
    ));

    assert_eq!(profile(&policy), Err(ProfileError::ControlCharacter));
}

#[test]
fn non_utf8_paths_are_rejected() {
    let mut policy = WritePolicy::default();
    policy.protect(PathBuf::from(OsStr::from_bytes(b"/Users/dev/\xff")));

    assert_eq!(profile(&policy), Err(ProfileError::NotUtf8));
}

#[test]
fn command_prepends_sandbox_exec_with_the_inline_profile() {
    let policy = policy();

    let command = command(OsStr::new("/opt/homebrew/bin/codex"), &policy).expect("command");

    let command = command.as_std();
    assert_eq!(command.get_program(), SANDBOX_EXEC);
    let arguments: Vec<&OsStr> = command.get_args().collect();
    assert_eq!(arguments.len(), 3);
    assert_eq!(arguments[0], "-p");
    assert_eq!(
        arguments[1],
        OsStr::new(&profile(&policy).expect("renderable policy"))
    );
    assert_eq!(arguments[2], "/opt/homebrew/bin/codex");
}

#[test]
fn unrenderable_policy_fails_the_launch() {
    let mut policy = WritePolicy::default();
    policy.allow_tree(PathBuf::from("/Users/dev/\u{7}"));

    assert_eq!(
        command(OsStr::new("/bin/sh"), &policy).err(),
        Some(AgentSessionError::Failed)
    );
}
