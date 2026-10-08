use std::collections::BTreeMap;
#[cfg(unix)]
use std::os::unix::process::CommandExt;

use super::*;

fn git(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn fetch_updates_and_prunes_origin_without_changing_checkout_or_worktree_files() {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "--bare", "remote"]);
    git(root.path(), &["init", "-b", "main", "source"]);
    let source = root.path().join("source");
    git(&source, &["config", "user.name", "Test"]);
    git(&source, &["config", "user.email", "test@example.invalid"]);
    std::fs::write(source.join("tracked"), "initial\n").unwrap();
    git(&source, &["add", "."]);
    git(&source, &["commit", "-m", "initial"]);
    git(&source, &["remote", "add", "origin", "../remote"]);
    git(&source, &["push", "origin", "main"]);
    git(&source, &["push", "origin", "main:obsolete"]);
    git(
        root.path(),
        &["clone", "--branch", "main", "remote", "checkout"],
    );
    let checkout = root.path().join("checkout");
    git(
        &checkout,
        &["worktree", "add", "-b", "feature", "../linked"],
    );
    let linked = root.path().join("linked");
    std::fs::write(checkout.join("tracked"), "dirty\n").unwrap();
    let head = git(&checkout, &["rev-parse", "HEAD"]);
    let index = std::fs::read(checkout.join(".git/index")).unwrap();
    std::fs::write(source.join("tracked"), "remote update\n").unwrap();
    git(&source, &["commit", "-am", "remote update"]);
    git(&source, &["push", "origin", "main", ":obsolete"]);
    let latest = git(&source, &["rev-parse", "HEAD"]);
    let backend = LocalGitFetch::new(root.path().join("managed"));
    assert_eq!(
        backend.repository(checkout.to_str().unwrap()).unwrap(),
        backend.repository(linked.to_str().unwrap()).unwrap()
    );
    backend
        .fetch(linked.to_str().unwrap(), &CancellationToken::new())
        .unwrap();
    assert_eq!(git(&checkout, &["rev-parse", "origin/main"]), latest);
    assert_eq!(git(&checkout, &["rev-parse", "HEAD"]), head);
    assert_eq!(git(&checkout, &["rev-parse", "main"]), head);
    assert_eq!(
        git(&checkout, &["for-each-ref", "refs/remotes/origin/obsolete"]),
        ""
    );
    assert_eq!(std::fs::read(checkout.join(".git/index")).unwrap(), index);
    assert_eq!(
        std::fs::read_to_string(checkout.join("tracked")).unwrap(),
        "dirty\n"
    );
    let status = backend.status(checkout.to_str().unwrap()).unwrap();
    assert_eq!(status.behind_of_origin, Some(1));
    // The existing Update-from-main action now consumes the freshly fetched reference.
    backend
        .checkout
        .merge_from_base(linked.to_str().unwrap(), Some("main"), true)
        .unwrap();
    assert_eq!(git(&linked, &["rev-parse", "HEAD"]), latest);
    assert_eq!(git(&checkout, &["rev-parse", "main"]), head);
    assert_eq!(
        backend
            .status(linked.to_str().unwrap())
            .unwrap()
            .ahead_behind,
        Some(crate::git::ports::checkout::AheadBehind {
            ahead: 0,
            behind: 0
        })
    );
    let commits = backend.checkout.commits(linked.to_str().unwrap()).unwrap();
    assert_eq!(commits.base_ref.as_deref(), Some("origin/main"));
    assert!(commits.commits.iter().all(|commit| commit.is_on_base));
    let diff = backend
        .checkout
        .diff(
            linked.to_str().unwrap(),
            &crate::git::ports::checkout::CheckoutDiffCompare {
                mode: crate::git::ports::checkout::CheckoutDiffMode::Base,
                base_ref: Some("main".to_owned()),
                ignore_whitespace: false,
            },
        )
        .unwrap();
    assert!(diff.files.is_empty());
}

#[test]
fn non_git_and_no_origin_directories_are_skipped() {
    let root = tempfile::tempdir().unwrap();
    let backend = LocalGitFetch::new(root.path().join("managed"));
    assert_eq!(
        backend.repository(root.path().to_str().unwrap()).unwrap(),
        None
    );
    git(root.path(), &["init", "--bare", "bare"]);
    let bare = root.path().join("bare");
    git(
        &bare,
        &["remote", "add", "origin", "https://example.invalid/repo"],
    );
    assert_eq!(backend.repository(bare.to_str().unwrap()).unwrap(), None);
    git(root.path(), &["init", "-b", "main"]);
    assert_eq!(
        backend.repository(root.path().to_str().unwrap()).unwrap(),
        None
    );
    git(
        root.path(),
        &["remote", "add", "upstream", "https://example.invalid/repo"],
    );
    assert_eq!(
        backend.repository(root.path().to_str().unwrap()).unwrap(),
        None
    );
}

#[test]
fn command_is_non_interactive_and_pre_cancelled_fetch_never_starts() {
    let command = fetch_command(".");
    let env: BTreeMap<_, _> = command
        .get_envs()
        .map(|(key, value)| (key.to_owned(), value.map(std::ffi::OsStr::to_owned)))
        .collect();
    assert_eq!(
        env.get(std::ffi::OsStr::new("GIT_TERMINAL_PROMPT"))
            .unwrap()
            .as_deref(),
        Some(std::ffi::OsStr::new("0"))
    );
    assert_eq!(
        env.get(std::ffi::OsStr::new("GCM_INTERACTIVE"))
            .unwrap()
            .as_deref(),
        Some(std::ffi::OsStr::new("never"))
    );
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let mut missing = Command::new("nonexistent-git-test-sentinel");
    assert_eq!(
        run_fetch(&mut missing, &cancellation, Duration::from_secs(1)),
        Err(GitFetchError::Cancelled)
    );
    assert_eq!(
        run_fetch(
            &mut missing,
            &CancellationToken::new(),
            Duration::from_secs(1)
        ),
        Err(GitFetchError::Failed)
    );
}

#[cfg(unix)]
#[test]
fn cancelled_and_timed_out_commands_are_reaped() {
    for cancel in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let pid = root.path().join("pid");
        let cancellation = CancellationToken::new();
        let task_cancel = cancellation.clone();
        let task_pid = pid.clone();
        let task = std::thread::spawn(move || {
            let mut command = Command::new("/bin/sh");
            command
                .process_group(0)
                .args(["-c", "echo $$ > \"$1\"; exec sleep 30", "test"])
                .arg(task_pid);
            run_fetch(
                &mut command,
                &task_cancel,
                if cancel {
                    Duration::from_secs(5)
                } else {
                    Duration::from_millis(100)
                },
            )
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while !pid.exists() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        if cancel {
            cancellation.cancel();
        }
        assert_eq!(
            task.join().unwrap(),
            Err(if cancel {
                GitFetchError::Cancelled
            } else {
                GitFetchError::TimedOut
            })
        );
        let child_pid = std::fs::read_to_string(pid).unwrap();
        assert!(
            !Command::new("/bin/kill")
                .args(["-0", child_pid.trim()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success()
        );
    }
}
