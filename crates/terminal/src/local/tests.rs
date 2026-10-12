use super::*;
use std::collections::BTreeMap;

mod io;
#[cfg(unix)]
mod paseo;

#[cfg(unix)]
fn process_is_running(pid: nix::unistd::Pid) -> bool {
    let output = std::process::Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    assert!(output.status.success() || output.status.code() == Some(1));
    let state = String::from_utf8(output.stdout).unwrap();
    // Group termination can leave an orphaned zombie awaiting adoption and reaping.
    !state.trim().is_empty() && !state.trim().starts_with('Z')
}

fn launch(root: &Path, script: &str) -> Launch {
    Launch {
        cwd: root.to_str().unwrap().to_owned(),
        command: Some("/bin/sh".to_owned()),
        args: vec!["-c".to_owned(), script.to_owned()],
        size: Size::default(),
        env: BTreeMap::new(),
    }
}

fn until(process: &dyn Process, predicate: impl Fn(&str) -> bool) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let output = process.capture().unwrap().join("\n");
        if predicate(&output) {
            return output;
        }
        assert!(Instant::now() < deadline, "PTY output timeout: {output}");
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
#[test]
fn startup_device_query_completes_before_the_first_subscription() {
    let temp = tempfile::tempdir().unwrap();
    let config = launch(
        temp.path(),
        r#"stty raw -echo min 0 time 10
printf '\033[c'
reply=$(dd bs=1 count=7 2>/dev/null)
if [ "$reply" = "$(printf '\033[?1;0c')" ]; then
    printf 'QUERY_READY'
else
    printf 'QUERY_FAILED'
fi"#,
    );
    let mut process = LocalRuntime.spawn(&config).unwrap();
    let output = until(process.as_ref(), |output| {
        output.contains("QUERY_READY") || output.contains("QUERY_FAILED")
    });
    assert!(output.contains("QUERY_READY"), "{output}");
    // Observers attach only after the child received its reply.
    process.observe(None, None).unwrap();
    process.observe(None, None).unwrap();
    process.kill().unwrap();
}

#[cfg(unix)]
#[test]
#[ignore = "requires fish 4 on PATH; run explicitly for shell compatibility validation"]
fn fish_interactive_startup_does_not_wait_for_a_display() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = launch(temp.path(), "");
    config.command = Some("fish".to_owned());
    config.args = vec![
        "--no-config".to_owned(),
        "--interactive".to_owned(),
        "--init-command".to_owned(),
        "function fish_prompt; printf FISH_READY; end".to_owned(),
    ];
    let mut process = LocalRuntime.spawn(&config).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let output = process.capture().unwrap().join("\n");
        assert!(!output.contains("could not read response"), "{output}");
        if output.contains("FISH_READY") {
            break;
        }
        assert!(Instant::now() < deadline, "fish startup stalled: {output}");
        thread::sleep(Duration::from_millis(10));
    }
    process.kill().unwrap();
}

#[cfg(unix)]
#[test]
fn native_pty_supports_tty_input_resize_capture_and_drop_cleanup() {
    let temp = tempfile::tempdir().unwrap();
    let config = launch(
        temp.path(),
        "test -t 0 && printf 'TTY_READY\\n'; while read line; do stty size; printf 'RESULT:%s\\n' \"$line\"; done",
    );
    let mut process = LocalRuntime.spawn(&config).unwrap();
    until(process.as_ref(), |output| output.contains("TTY_READY"));
    process
        .send(&Input::Resize(crate::protocol::Resize {
            size: Size { rows: 30, cols: 90 },
            intent: crate::protocol::ResizeIntent::Claim,
        }))
        .unwrap();
    process
        .send(&Input::Input {
            data: "hello\r".to_owned(),
        })
        .unwrap();
    let text = until(process.as_ref(), |output| output.contains("RESULT:hello"));
    assert!(text.contains("30 90"));
    assert!(!process.exited().unwrap());
    assert!(!process.observe(None, None).unwrap().frames.is_empty());
    assert_eq!(
        process.send(&Input::Input {
            data: "x".repeat(65537)
        }),
        Err(Error::Exhausted)
    );
    process.kill().unwrap();
    process.kill().unwrap();
    assert!(process.exited().unwrap());
}

#[cfg(unix)]
#[test]
fn same_size_claim_preserves_the_terminal_output_stream() {
    let temp = tempfile::tempdir().unwrap();
    let config = launch(
        temp.path(),
        "printf 'READY\\n'; while read line; do printf 'RESULT:%s\\n' \"$line\"; done",
    );
    let mut process = LocalRuntime.spawn(&config).unwrap();
    until(process.as_ref(), |output| output.contains("READY"));
    let initial = process.observe(None, None).unwrap();

    process
        .send(&Input::Resize(crate::protocol::Resize {
            size: config.size,
            intent: crate::protocol::ResizeIntent::Claim,
        }))
        .unwrap();

    let unchanged = process.observe(Some(initial.revision), None).unwrap();
    assert_eq!(unchanged.size, config.size);
    assert_eq!(unchanged.revision, initial.revision);
    assert!(unchanged.frames.is_empty());

    process
        .send(&Input::Input {
            data: "next\r".to_owned(),
        })
        .unwrap();
    until(process.as_ref(), |output| output.contains("RESULT:next"));
    let output = process.observe(Some(initial.revision), None).unwrap();
    assert!(!output.frames.is_empty());
    assert!(
        output
            .frames
            .iter()
            .all(|(opcode, _)| *opcode == crate::protocol::Opcode::Output)
    );
}

#[cfg(unix)]
#[test]
fn spawn_failure_natural_exit_and_environment_are_explicit() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = launch(temp.path(), "printf 'DONE:%s' \"$PASEO_WORKSPACE_ID\"");
    config
        .env
        .insert("PASEO_WORKSPACE_ID".to_owned(), "workspace".to_owned());
    let mut process = LocalRuntime.spawn(&config).unwrap();
    until(process.as_ref(), |output| output.contains("DONE:workspace"));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !process.exited().unwrap() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    assert!(process.observe(None, None).unwrap().exited);
    config.command = Some("/nonexistent/terminal-command".to_owned());
    assert!(matches!(LocalRuntime.spawn(&config), Err(Error::Io)));
    assert!(LocalRuntime.directory("relative").is_err());
    assert!(
        LocalRuntime
            .directory("/nonexistent/terminal-directory")
            .is_err()
    );
    assert_eq!(
        Path::new(
            &LocalRuntime
                .directory(temp.path().to_str().unwrap())
                .unwrap()
        ),
        temp.path().canonicalize().unwrap()
    );
}

#[cfg(unix)]
#[test]
fn kill_terminates_background_group_after_shell_has_already_exited() {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let temp = tempfile::tempdir().unwrap();
    let config = launch(
        temp.path(),
        "trap '' HUP; sleep 60 & printf 'CHILD:%s\\n' \"$!\"; exit",
    );
    let mut process = LocalRuntime.spawn(&config).unwrap();
    let output = until(process.as_ref(), |output| output.contains("CHILD:"));
    let pid = output
        .lines()
        .find_map(|line| line.strip_prefix("CHILD:"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let pid = Pid::from_raw(pid);
    assert!(process_is_running(pid));
    thread::sleep(Duration::from_millis(100));
    let _ = process.exited().unwrap();
    process.kill().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while process_is_running(pid) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    let survived = process_is_running(pid);
    if survived {
        let _ = kill(pid, Signal::SIGKILL);
    }
    assert!(!survived, "background child survived terminal kill");
    assert!(process.exited().unwrap());
}
