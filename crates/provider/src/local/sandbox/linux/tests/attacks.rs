use super::*;

#[tokio::test]
async fn linux_sandbox_blocks_filesystem_and_namespace_escape_attempts() {
    let root = scratch();
    let probe = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/linux_sandbox_probe.py");
    let setup = StdCommand::new("python3")
        .arg(&probe)
        .arg("prepare")
        .arg(root.path())
        .output()
        .expect("prepare writable control files");
    assert!(setup.status.success(), "{setup:?}");
    let home = root.path().join("home");
    let worktree = root.path().join("wt");
    let confinement = super::super::super::tests::confinement(&worktree);
    let output = super::super::super::command(
        Path::new("/usr/bin/python3"),
        Some(&confinement),
        super::super::super::CODEX_STATE,
        &|name| (name == "HOME").then(|| home.display().to_string()),
    )
    .expect("sandbox command")
    .arg(&probe)
    .arg("attack")
    .arg(root.path())
    .current_dir(&worktree)
    .output()
    .await
    .expect("run filesystem probes");
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("probe report");
    assert_eq!(report["unexpected"], serde_json::json!([]), "{report:#}");
    assert_eq!(
        report["denied"].as_array().expect("denied").len(),
        28,
        "{report:#}"
    );
    assert_eq!(
        report["allowed"].as_array().expect("allowed").len(),
        3,
        "{report:#}"
    );
    // Re-resolve the next launch's policy, which must not have gained the home tree.
    let next =
        super::super::super::policy(&confinement, super::super::super::CODEX_STATE, &|name| {
            (name == "HOME").then(|| home.display().to_string())
        })
        .expect("next launch policy");
    assert!(!next.trees().iter().any(|tree| home.starts_with(tree)));
}

#[test]
fn linux_sandbox_detaches_from_the_launchers_controlling_terminal() {
    let driver =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/linux_sandbox_terminal.py");
    let sandbox =
        command(OsStr::new("/usr/bin/python3"), &WritePolicy::default()).expect("sandbox command");
    let probe = "import errno, os
try:
    os.close(os.open('/dev/tty', os.O_RDWR))
except OSError as error:
    assert error.errno == errno.ENXIO, error
else:
    raise AssertionError('inherited controlling terminal')";
    // The same command without --new-session must expose the controlling terminal;
    // this proves the regression test does not pass merely because CI has no TTY.
    for detach in [false, true] {
        let output = StdCommand::new("python3")
            .arg(&driver)
            .arg(sandbox.as_std().get_program())
            .args(
                sandbox
                    .as_std()
                    .get_args()
                    .filter(|arg| detach || *arg != "--new-session"),
            )
            .args(["-c", probe])
            .output()
            .expect("PTY probe");
        assert_eq!(output.status.success(), detach, "{output:?}");
    }
}
