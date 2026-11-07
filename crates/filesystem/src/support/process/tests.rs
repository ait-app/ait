use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;

use super::{
    Bounds, RunError, configured_ssh_command, disable_git_prompts, git_command, git_program,
    run_bounded,
};

const BOUNDS: Bounds = Bounds {
    timeout: Duration::from_secs(5),
    stdout_limit: 1024,
    stderr_limit: 8,
};

#[test]
fn completed_commands_return_status_and_truncated_diagnostics() {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "printf out; printf 'diagnostic text' >&2; exit 3"]);

    let output = run_bounded(&mut command, BOUNDS, None).unwrap();

    assert_eq!(output.code(), 3);
    assert_eq!(output.stdout, b"out");
    assert_eq!(output.stderr, b"diagnost");
}

#[test]
fn commands_that_finish_after_overflowing_stderr_still_complete() {
    // The overflow is observed while the shell is still running, well before it exits.
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "printf 'diagnostic text' >&2; sleep 0.1; exit 3"]);

    let output = run_bounded(&mut command, BOUNDS, None).unwrap();

    assert_eq!(output.code(), 3);
    assert_eq!(output.stderr, b"diagnost");
}

#[test]
fn commands_that_keep_running_after_overflowing_stderr_are_stopped() {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "printf 'diagnostic text' >&2; sleep 30"]);
    let started = Instant::now();

    let error = run_bounded(&mut command, BOUNDS, None).unwrap_err();

    assert!(matches!(error, RunError::StderrExceeded), "{error:?}");
    assert!(started.elapsed() < BOUNDS.timeout);
}

#[test]
fn pre_cancelled_commands_never_start() {
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let mut command = Command::new("nonexistent-process-test-sentinel");

    let error = run_bounded(&mut command, BOUNDS, Some(&cancellation)).unwrap_err();

    assert!(matches!(error, RunError::Cancelled));
    assert!(matches!(
        run_bounded(&mut command, BOUNDS, None).unwrap_err(),
        RunError::Spawn(_)
    ));
}

#[cfg(unix)]
#[test]
fn timeouts_terminate_the_whole_process_group_gracefully() {
    let root = tempfile::tempdir().unwrap();
    let grandchild = root.path().join("grandchild");
    let terminated = root.path().join("terminated");
    let mut command = Command::new("/bin/sh");
    // The shell records SIGTERM, as Git does to release its locks, and leaves a grandchild
    // that would otherwise outlive the killed shell.
    command
        .args([
            "-c",
            "trap 'echo term > \"$2\"; exit 1' TERM; sleep 30 & echo $! > \"$1\"; wait",
            "test",
        ])
        .arg(&grandchild)
        .arg(&terminated);
    let started = Instant::now();

    let error = run_bounded(
        &mut command,
        Bounds {
            timeout: Duration::from_millis(200),
            ..BOUNDS
        },
        None,
    )
    .unwrap_err();

    assert!(matches!(error, RunError::TimedOut));
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(std::fs::read_to_string(&terminated).unwrap().trim(), "term");
    let pid = std::fs::read_to_string(&grandchild).unwrap();
    // The orphaned grandchild is reaped by init shortly after it dies.
    let deadline = Instant::now() + Duration::from_secs(5);
    while Command::new("/bin/kill")
        .args(["-0", pid.trim()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
    {
        assert!(Instant::now() < deadline, "grandchild {pid} survived");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn network_git_commands_use_batch_ssh_unless_the_repository_configures_one() {
    let root = tempfile::tempdir().unwrap();
    let repository = root.path();
    assert!(
        git_command(repository)
            .arg("init")
            .status()
            .unwrap()
            .success()
    );
    let environment = |arguments: &[&str]| {
        let mut command = git_command(repository);
        disable_git_prompts(&mut command, repository, arguments);
        let lookup = |name: &str| {
            command
                .get_envs()
                .find(|(key, _)| *key == name)
                .and_then(|(_, value)| value)
                .map(|value| value.to_string_lossy().into_owned())
        };
        (lookup("GIT_SSH_COMMAND"), lookup("SSH_ASKPASS_REQUIRE"))
    };
    // A user-level core.sshCommand on the test machine is honored like a repository one.
    let inherited = configured_ssh_command(repository);

    let (ssh, askpass) = environment(&["fetch", "origin"]);
    assert_eq!(ssh.is_none(), inherited);
    if !inherited {
        assert_eq!(ssh.as_deref(), Some("ssh -oBatchMode=yes"));
    }
    assert_eq!(askpass.as_deref(), Some("never"));
    assert_eq!(environment(&["status"]).0, None);

    assert!(
        git_command(repository)
            .args(["config", "core.sshCommand", "ssh -i key"])
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(environment(&["push", "origin"]).0, None);
}

#[test]
fn git_program_runs_git_without_the_macos_toolchain_shim() {
    let program = git_program();
    if cfg!(target_os = "macos") {
        assert_ne!(program, std::path::Path::new("/usr/bin/git"));
    }
    let output = Command::new(program).arg("--version").output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("git version"));
}

#[test]
fn slow_commands_complete_while_polling_backs_off() {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "sleep 0.3; printf done"]);
    let started = Instant::now();

    let output = run_bounded(&mut command, BOUNDS, None).unwrap();

    assert_eq!(output.stdout, b"done");
    // Polling is capped, so completion is noticed within one maximum interval.
    assert!(started.elapsed() < Duration::from_millis(300) + Duration::from_secs(1));
}
