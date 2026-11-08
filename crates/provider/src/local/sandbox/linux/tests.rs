use std::fs;
use std::path::Path;
use std::process::Command as StdCommand;

use super::*;

fn scratch() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(".linux-sandbox-test-")
        .tempdir_in(std::env::current_dir().expect("cwd"))
        .expect("scratch")
}

fn git(directory: &Path, args: &[&str]) {
    let output = StdCommand::new("git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("git");
    assert!(output.status.success(), "{output:?}");
}

#[test]
fn missing_protected_entries_are_initialized_without_overwriting_existing_content() {
    let root = tempfile::tempdir().expect("root");
    let config = root.path().join("settings.json");
    protect(&config).expect("guard");
    assert_eq!(fs::read_to_string(&config).expect("json"), "{}\n");
    fs::write(&config, "{\"existing\":true}").expect("existing config");
    protect(&config).expect("existing guard");
    assert_eq!(
        fs::read_to_string(&config).expect("json"),
        "{\"existing\":true}"
    );
    let hooks = root.path().join("hooks");
    protect(&hooks).expect("directory guard");
    assert!(hooks.is_dir());
    let toml = root.path().join("config.toml");
    protect(&toml).expect("toml guard");
    assert!(fs::read(&toml).expect("toml").is_empty());
    let dangling = root.path().join("dangling");
    std::os::unix::fs::symlink(root.path().join("missing"), &dangling).expect("symlink");
    assert!(protect(&dangling).is_err());
    assert!(!root.path().join("missing").exists());
    let alias = root.path().join("alias");
    std::os::unix::fs::symlink(&config, &alias).expect("existing symlink");
    assert!(protect(&alias).is_err());
}

#[test]
fn unsupported_prefixes_fail_before_modifying_the_filesystem() {
    let root = tempfile::tempdir().expect("root");
    let mut policy = WritePolicy::default();
    policy.allow_tree(root.path().join("state"));
    policy.allow_prefix(root.path().join(".claude.json"));
    assert_eq!(
        prepare(&policy).expect_err("prefix rejected").kind(),
        io::ErrorKind::Unsupported
    );
    assert!(!root.path().join("state").exists());
}

#[test]
fn claude_keeps_credentials_in_place_and_seeds_global_state_only_once() {
    let home = tempfile::tempdir().expect("home");
    let lookup = |name: &str| (name == "HOME").then(|| home.path().display().to_string());
    fs::write(home.path().join(".claude.json"), "{\"fixture\":true}").expect("state");
    let directory = claude_config_dir(&lookup).expect("config dir");
    assert_eq!(directory, home.path().join(".claude"));
    let state = directory.join(".claude.json");
    assert_eq!(
        fs::read_to_string(&state).expect("seeded"),
        "{\"fixture\":true}"
    );
    fs::write(&state, "{\"updated\":true}").expect("update");
    claude_config_dir(&lookup).expect("reuse");
    assert_eq!(
        fs::read_to_string(&state).expect("preserved"),
        "{\"updated\":true}"
    );
    let explicit = |name: &str| (name == "CLAUDE_CONFIG_DIR").then(|| "/custom/claude".to_owned());
    assert_eq!(
        claude_config_dir(&explicit).expect("explicit"),
        Path::new("/custom/claude")
    );
    assert!(claude_config_dir(&|_| None).is_err());
    let empty = tempfile::tempdir().expect("empty home");
    assert!(
        claude_config_dir(&|name| (name == "HOME").then(|| empty.path().display().to_string()))
            .is_ok()
    );
}

#[tokio::test]
async fn linux_sandbox_enforces_writes_preserves_auth_and_allows_git_commit() {
    let root = scratch();
    let repo = root.path().join("repo");
    let worktree = root.path().join("wt");
    let home = root.path().join("home");
    fs::create_dir_all(&repo).expect("repo");
    fs::create_dir_all(home.join(".codex")).expect("state");
    git(&repo, &["init", "-q"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=Sandbox Test",
            "-c",
            "user.email=sandbox@example.invalid",
            "commit",
            "--allow-empty",
            "-qm",
            "initial",
        ],
    );
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-qb",
            "sandbox",
            worktree.to_str().expect("worktree path"),
        ],
    );
    let auth = home.join(".codex/auth.json");
    fs::write(&auth, "fixture-auth").expect("auth fixture");
    std::os::unix::fs::symlink(&home, worktree.join("outside")).expect("escape symlink");
    let confinement = super::super::tests::confinement(&worktree);
    let lookup = |name: &str| (name == "HOME").then(|| home.display().to_string());
    let mut process = super::super::command(
        Path::new("/bin/sh"),
        Some(&confinement),
        super::super::CODEX_STATE,
        &lookup,
    )
    .expect("command");
    let output = process
        .current_dir(&worktree)
        .env("HOME", &home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("SANDBOX_FIXTURE_KEY", "inherited-fixture")
        .arg("-c")
        .arg(
            r#"
set -eu
[ "$SANDBOX_FIXTURE_KEY" = inherited-fixture ]
[ "$(cat "$HOME/.codex/auth.json")" = fixture-auth ]
printf refreshed > "$HOME/.codex/auth.json.tmp"
mv "$HOME/.codex/auth.json.tmp" "$HOME/.codex/auth.json"
printf ok > allowed
if (printf no > "$HOME/forbidden"); then exit 10; fi
if (printf no > outside/forbidden); then exit 11; fi
if (printf no > ../forbidden); then exit 12; fi
if (printf no > "$HOME/.codex/config.toml"); then exit 13; fi
if rm "$HOME/.codex/config.toml"; then exit 14; fi
if git config forbidden.key value; then exit 15; fi
if (printf no > .git); then exit 16; fi
private_git=$(git rev-parse --absolute-git-dir)
if (printf no > "$private_git/config.worktree"); then exit 17; fi
if mv "$private_git" "$private_git-moved"; then exit 18; fi
if mv "$(dirname "$private_git")" "$(dirname "$private_git")-moved"; then exit 19; fi
git add allowed
git -c user.name='Sandbox Test' -c user.email=sandbox@example.invalid commit -qm sandbox
printf ok > /dev/null
scratch=$(mktemp)
printf ok > "$scratch"
rm "$scratch"
"#,
        )
        .output()
        .await
        .expect("bubblewrap launched");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        fs::read_to_string(auth).expect("refreshed auth"),
        "refreshed"
    );
    assert_eq!(
        fs::read_to_string(worktree.join("allowed")).expect("write"),
        "ok"
    );
    assert!(!home.join("forbidden").exists());
    assert!(!root.path().join("forbidden").exists());
}

#[tokio::test]
async fn claude_global_settings_can_be_replaced_atomically_without_home_writes() {
    let root = scratch();
    let worktree = root.path().join("wt");
    let home = root.path().join("home");
    fs::create_dir_all(worktree.join(".git")).expect("checkout");
    fs::create_dir_all(&home).expect("home");
    fs::write(home.join(".claude.json"), "{\"fixture\":true}").expect("global state");
    let lookup = |name: &str| (name == "HOME").then(|| home.display().to_string());
    let config = claude_config_dir(&lookup).expect("config");
    let lookup = |name: &str| match name {
        "HOME" => Some(home.display().to_string()),
        "CLAUDE_CONFIG_DIR" => Some(config.display().to_string()),
        _ => None,
    };
    let confinement = super::super::tests::confinement(&worktree);
    let output = super::super::command(
        Path::new("/bin/sh"),
        Some(&confinement),
        super::super::CLAUDE_STATE,
        &lookup,
    )
    .expect("sandbox")
    .env("CLAUDE_CONFIG_DIR", &config)
    .env("HOME", &home)
    .arg("-c")
    .arg(
        r#"
set -eu
printf '{}' > "$CLAUDE_CONFIG_DIR/.claude.json.tmp.fixture"
mv "$CLAUDE_CONFIG_DIR/.claude.json.tmp.fixture" "$CLAUDE_CONFIG_DIR/.claude.json"
if (printf no > "$HOME/.claude.json"); then exit 20; fi
if (printf no > "$CLAUDE_CONFIG_DIR/settings.json"); then exit 21; fi
if (printf no > "$CLAUDE_CONFIG_DIR/plugins/forbidden"); then exit 22; fi
"#,
    )
    .output()
    .await
    .expect("shell");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        fs::read_to_string(config.join(".claude.json")).expect("state"),
        "{}"
    );
    assert_eq!(
        fs::read_to_string(home.join(".claude.json")).expect("original"),
        "{\"fixture\":true}"
    );
}

#[tokio::test]
async fn linux_sandbox_keeps_host_loopback_network_access() {
    use tokio::io::AsyncWriteExt as _;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("connection");
        stream.write_all(b"ok").await.expect("reply");
    });
    let output = command(OsStr::new("python3"), &WritePolicy::default())
        .expect("sandbox")
        .args([
            "-c",
            "import socket, sys
s = socket.create_connection(('127.0.0.1', int(sys.argv[1])), timeout=3)
assert s.recv(2) == b'ok'",
            &address.port().to_string(),
        ])
        .output()
        .await
        .expect("network probe");
    server.abort();
    assert!(output.status.success(), "{output:?}");
}
