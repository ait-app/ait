//! Supervisor behavior: whole-group termination and bounded reaping.
//!
//! These run real child processes because the behavior under test is the signal and wait
//! sequence, which no in-process double reproduces.

use super::*;

/// Budget used by these tests so a timeout does not cost the suite a minute.
const TEST_BUDGET: Budget = Budget {
    timeout: Duration::from_millis(600),
    grace: Duration::from_secs(5),
};

/// Write a fake installer (a POSIX shell script) and return its path.
///
/// The supervisor invokes `<node> <installer> --data-dir <dir>`; under `/bin/sh` the script is
/// `$0`, so the data directory is `$2`. These tests use a real interpreter because the behavior
/// under test is the signal and wait sequence, which no in-process double reproduces.
fn installer(directory: &Path, script: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = directory.join("install.mjs");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// Whether `pid` is still observable by `/proc`. A zombie is reported as gone by `terminate`.
fn running(pid: i32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => {
            let state = stat
                .rsplit_once(") ")
                .and_then(|(_, tail)| tail.split(' ').next())
                .unwrap_or_default();
            state != "Z" && state != "X"
        }
        Err(_) => false,
    }
}

/// Wait up to `limit` for `pid` to disappear, so the parent's reap is not raced.
fn gone(pid: i32, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if !running(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    !running(pid)
}

#[test]
fn a_timed_out_installer_terminates_its_whole_process_group() {
    let directory = tempfile::tempdir().unwrap();
    // The script starts a grandchild standing in for `npm ci`, records both pids and blocks.
    let script = installer(
        directory.path(),
        "#!/bin/sh\nsleep 300 &\necho $! > \"$2/grandchild.pid\"\necho $$ > \"$2/installer.pid\"\nwait\n",
    );
    workspace_within(
        directory.path(),
        &|name| match name {
            "AIT_SERVER_OMP_NATIVE_HOST_INSTALLER" => Some(script.clone().into_os_string()),
            // /bin/sh accepts the installer path as an unused argument.
            "AIT_SERVER_NODE_BIN" => Some("/bin/sh".into()),
            _ => None,
        },
        TEST_BUDGET,
    );
    let grandchild: i32 = std::fs::read_to_string(directory.path().join("grandchild.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let installer_pid: i32 = std::fs::read_to_string(directory.path().join("installer.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    // Both the grandchild and the group leader are gone: the group was signaled, not just the pid.
    assert!(
        gone(grandchild, Duration::from_secs(5)),
        "grandchild survived"
    );
    assert!(
        gone(installer_pid, Duration::from_secs(5)),
        "installer group leader survived"
    );
}

#[test]
fn an_installer_that_exits_cleanly_is_reaped_without_being_signaled() {
    let directory = tempfile::tempdir().unwrap();
    let script = installer(
        directory.path(),
        "#!/bin/sh\necho done > \"$2/finished\"\nexit 0\n",
    );
    workspace_within(
        directory.path(),
        &|name| match name {
            "AIT_SERVER_OMP_NATIVE_HOST_INSTALLER" => Some(script.clone().into_os_string()),
            "AIT_SERVER_NODE_BIN" => Some("/bin/sh".into()),
            _ => None,
        },
        TEST_BUDGET,
    );
    // The script reached its last statement instead of being killed at the deadline.
    assert_eq!(
        std::fs::read_to_string(directory.path().join("finished"))
            .unwrap()
            .trim(),
        "done"
    );
    // No managed tree appeared, so the run ends in the documented degradation only.
    assert!(
        !directory
            .path()
            .join("providers/omp-native-host/1")
            .exists()
    );
}

#[test]
fn a_successful_installer_publishes_a_runtime_the_probe_then_adopts() {
    let directory = tempfile::tempdir().unwrap();
    // A stand-in for install.mjs that publishes a complete, verifiable tree.
    let script = installer(
        directory.path(),
        r#"#!/bin/sh
root="$2/providers/omp-native-host/1"
sdk="$root/node_modules/@oh-my-pi/pi-coding-agent"
mkdir -p "$root/node_modules/@oven/bun-linux-x64/bin" "$sdk/src/modes/rpc"
: > "$root/node_modules/@oven/bun-linux-x64/bin/bun"
: > "$sdk/src/cli.ts"
: > "$sdk/src/modes/rpc/ait-native-host-hello.ts"
cat > "$root/manifest.json" <<'JSON'
{"schemaVersion":1,"adapter":"ait-omp-native-host","adapterVersion":1,
"ompVersion":"18.17.0","bunVersion":"1.4.4",
"bunPath":"node_modules/@oven/bun-linux-x64/bin/bun",
"entrypoint":"node_modules/@oh-my-pi/pi-coding-agent/src/cli.ts"}
JSON
"#,
    );
    let environment = |name: &str| match name {
        "AIT_SERVER_OMP_NATIVE_HOST_INSTALLER" => Some(script.clone().into_os_string()),
        "AIT_SERVER_NODE_BIN" => Some("/bin/sh".into()),
        _ => None,
    };
    // Nothing verifies before the run, and the installation is visible immediately after it.
    assert!(discover(directory.path(), &environment).is_none());
    workspace_within(directory.path(), &environment, TEST_BUDGET);
    assert!(discover(directory.path(), &environment).is_some());
}

#[test]
fn a_stalled_installer_does_not_hold_the_probe_past_its_budget() {
    let directory = tempfile::tempdir().unwrap();
    let script = installer(directory.path(), "#!/bin/sh\nsleep 300\n");
    let started = Instant::now();
    workspace_within(
        directory.path(),
        &|name| match name {
            "AIT_SERVER_OMP_NATIVE_HOST_INSTALLER" => Some(script.clone().into_os_string()),
            "AIT_SERVER_NODE_BIN" => Some("/bin/sh".into()),
            _ => None,
        },
        TEST_BUDGET,
    );
    // The probe returns after the budget and its bounded grace, not when the script would end.
    assert!(started.elapsed() < Duration::from_secs(30));
}
