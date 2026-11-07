//! Ensure the OMP native host runtime is discoverable before OMP sessions are created.
//!
//! Two installation shapes are supported and neither blocks startup:
//!
//! * **Bundled** — a packaged desktop app or standalone daemon ships
//!   `provider-runtimes/omp-native-host/1` next to its binary; discovery finds it in place, so
//!   nothing is copied into user state.
//! * **Managed** — `<data-dir>/providers/omp-native-host/1` is produced by
//!   `runtimes/omp-native-host/install.mjs`. When that tree is absent and an installer is
//!   available (`AIT_SERVER_OMP_NATIVE_HOST_INSTALLER`, otherwise next to a development
//!   checkout's runtime sources), the daemon runs it once and reports the outcome.
//!
//! A missing runtime only degrades `/vibe` to an explicit refusal card; it never stops the
//! daemon. The installer owns the `.install-lock` file and reclaims one whose recorded holder is
//! gone, so a killed or timed-out run cannot wedge every later daemon start.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use provider::native_host::discover;

/// Managed installation root inside the daemon data directory.
const MANAGED: &str = "providers/omp-native-host/1/manifest.json";
/// Installer entry point relative to the repository root in a development checkout.
const SOURCE_INSTALLER: &str = "runtimes/omp-native-host/install.mjs";
/// Upper bound on one installer run.
///
/// A cold `npm ci` on a slow link legitimately takes minutes, so the budget is generous; a run
/// that overruns it is terminated and the next daemon start tries again.
const INSTALL_TIMEOUT: Duration = Duration::from_secs(300);
/// Time an installer's process group gets to exit before the whole group is killed outright.
const TERMINATE_GRACE: Duration = Duration::from_secs(5);
/// Poll interval while the installer runs, so termination never waits on a blocking call.
const POLL: Duration = Duration::from_millis(50);
/// Bytes of installer output kept for one diagnostic line.
const OUTPUT_TAIL: usize = 512;

/// One installer run's time budget.
#[derive(Debug, Clone, Copy)]
struct Budget {
    /// Total time the installer may run before its process group is terminated.
    timeout: Duration,
    /// Time the group gets to exit after `SIGTERM` before it is killed.
    grace: Duration,
}

const BUDGET: Budget = Budget {
    timeout: INSTALL_TIMEOUT,
    grace: TERMINATE_GRACE,
};

/// How one supervised installer run ended.
#[derive(Debug)]
enum Completion {
    /// The installer process exited with this status.
    Exited(ExitStatus),
    /// The installer was still running when its budget ran out.
    TimedOut,
}

/// Warn that an installation attempt failed: what happened, the user-visible impact and when
/// the installer runs again.
///
/// Retrying is deliberately unconditional on the next daemon start rather than tied to a manifest
/// change: a first install usually has no manifest yet, so a change-triggered retry could never
/// fire and the runtime would stay unavailable forever.
fn degraded(reason: impl std::fmt::Display) {
    tracing::warn!(
        "OMP native host runtime unavailable: {reason}. OMP keeps the official binary, so only \
         /vibe is unavailable; the installer runs again on the next daemon start"
    );
}

/// Warn that an existing installation failed verification and is left alone.
///
/// Reinstalling here cannot decide which tree wins, so the documented degradation stands until a
/// new runtime is installed or the current one is repaired.
fn degraded_permanently(reason: impl std::fmt::Display) {
    tracing::warn!(
        "OMP native host runtime unavailable: {reason}. OMP keeps the official binary, so only \
         /vibe is unavailable; the installer will not reinstall over an unverified tree"
    );
}

/// Resolve the runtime for `data_dir`, installing the managed copy when it is absent.
///
/// The probe exists so an operator sees the degradation in the daemon log instead of discovering
/// it only when `/vibe` is refused. The provider composition rediscovers the runtime on its own.
pub(crate) fn ensure_runtime(data_dir: &Path) {
    workspace(data_dir, &|name| std::env::var_os(name));
}

/// Probe with an explicit environment so startup and tests resolve identically.
pub(super) fn workspace(data_dir: &Path, environment: &dyn Fn(&str) -> Option<std::ffi::OsString>) {
    workspace_within(data_dir, environment, BUDGET);
}

/// Probe and install with an explicit budget so tests can exercise the timeout path.
fn workspace_within(
    data_dir: &Path,
    environment: &dyn Fn(&str) -> Option<std::ffi::OsString>,
    budget: Budget,
) {
    if let Some(host) = discover(data_dir, environment) {
        tracing::info!(
            entrypoint = %host.entrypoint().display(),
            "OMP native host runtime available"
        );
        return;
    }
    install(data_dir, environment, budget);
    if let Some(host) = discover(data_dir, environment) {
        tracing::info!(
            entrypoint = %host.entrypoint().display(),
            "OMP native host runtime installed"
        );
    } else {
        degraded(format_args!(
            "{} did not verify after the installer run",
            data_dir.join(MANAGED).display()
        ));
    }
}

/// Run the installer once when a managed tree is missing and an installer is reachable.
///
/// Any failure is a warning: the daemon keeps serving with the official OMP binary.
fn install(
    data_dir: &Path,
    environment: &dyn Fn(&str) -> Option<std::ffi::OsString>,
    budget: Budget,
) {
    let managed = data_dir.join(MANAGED);
    if managed.exists() {
        // A manifest exists but did not verify; re-running the installer cannot decide which
        // tree wins, so the verified degradation is reported instead.
        degraded_permanently(format_args!(
            "the installed manifest {} failed verification",
            managed.display()
        ));
        return;
    }
    let Some(installer) = installer_path(environment) else {
        tracing::debug!("No OMP native host installer is available beside this daemon");
        return;
    };
    let Some(node) = environment("AIT_SERVER_NODE_BIN").map(PathBuf::from) else {
        degraded(format_args!(
            "{} needs Node.js; set AIT_SERVER_NODE_BIN to install it",
            installer.display()
        ));
        return;
    };
    run(&node, &installer, data_dir, budget);
}

/// Start the installer in its own process group and require a clean exit within `budget`.
///
/// The installer shells out to `npm ci`, which starts its own children, so the daemon owns a
/// whole group instead of a single pid: a timeout then terminates npm too rather than orphaning
/// it. Output goes to a temporary file because a piped stderr nobody drains can fill up and
/// deadlock the installer long before the deadline.
fn run(node: &Path, installer: &Path, data_dir: &Path, budget: Budget) {
    let Ok(log) = tempfile::tempfile() else {
        degraded("the installer log could not be created");
        return;
    };
    let Ok(stderr) = log.try_clone() else {
        degraded("the installer log could not be shared");
        return;
    };
    let mut command = Command::new(node);
    command
        .arg(installer)
        .arg("--data-dir")
        .arg(data_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            degraded(format_args!("the installer could not start: {error}"));
            return;
        }
    };
    match wait_for(&mut child, Instant::now() + budget.timeout) {
        Ok(Completion::Exited(status)) if status.success() => {}
        Ok(Completion::Exited(status)) => {
            degraded(format_args!(
                "the installer exited with {status}{}",
                installer_output(&log)
            ));
        }
        Ok(Completion::TimedOut) => {
            // Read the log after the group is gone, so the tail covers the whole run.
            terminate(&mut child, budget.grace);
            degraded(format_args!(
                "the installer exceeded its {}s budget and its process group was terminated{}",
                budget.timeout.as_secs(),
                installer_output(&log)
            ));
        }
        Err(error) => {
            terminate(&mut child, budget.grace);
            degraded(format_args!(
                "the installer could not be observed: {error}{}",
                installer_output(&log)
            ));
        }
    }
}

/// Wait for the installer until `deadline`, reaping it as soon as it exits.
fn wait_for(child: &mut Child, deadline: Instant) -> std::io::Result<Completion> {
    loop {
        match child.try_wait()? {
            Some(status) => return Ok(Completion::Exited(status)),
            None if Instant::now() >= deadline => return Ok(Completion::TimedOut),
            None => std::thread::sleep(POLL),
        }
    }
}

/// Wait at most `limit` for the direct child to exit.
fn wait_within(child: &mut Child, limit: Duration) -> Option<ExitStatus> {
    match wait_for(child, Instant::now() + limit) {
        Ok(Completion::Exited(status)) => Some(status),
        Ok(Completion::TimedOut) | Err(_) => None,
    }
}

/// Stop the installer's whole process group and reap the direct child.
///
/// `SIGTERM` first so npm can clean up, then `SIGKILL` for a group that ignores it. Because the
/// installer is a group leader, signaling the group also stops the processes it started; the
/// direct child is always waited, so no zombie is left behind.
fn terminate(child: &mut Child, grace: Duration) {
    #[cfg(unix)]
    let group = child.id();
    #[cfg(unix)]
    signal_group(group, rustix::process::Signal::TERM);
    #[cfg(not(unix))]
    let _ = child.kill();
    if wait_within(child, grace).is_some() {
        return;
    }
    #[cfg(unix)]
    signal_group(group, rustix::process::Signal::KILL);
    if wait_within(child, grace).is_none() {
        // A leader that survived `SIGKILL` (uninterruptible sleep) still has to be reaped.
        let _ = child.wait();
    }
}

/// Deliver `signal` to process group `group`, ignoring a group that is already gone.
#[cfg(unix)]
fn signal_group(group: u32, signal: rustix::process::Signal) {
    if let Ok(group) = i32::try_from(group)
        && let Some(group) = rustix::process::Pid::from_raw(group)
    {
        let _ = rustix::process::kill_process_group(group, signal);
    }
}

/// Last bytes of the installer log, formatted for a warning when the installer failed.
fn installer_output(log: &std::fs::File) -> String {
    let Ok(length) = log.metadata().map(|metadata| metadata.len()) else {
        return String::new();
    };
    let limit = OUTPUT_TAIL as u64;
    let start = length.saturating_sub(limit);
    let mut reader = log;
    let mut bytes = Vec::with_capacity(OUTPUT_TAIL);
    if reader.seek(SeekFrom::Start(start)).is_err()
        || reader.take(limit).read_to_end(&mut bytes).is_err()
    {
        return String::new();
    }
    let text = String::from_utf8_lossy(&bytes);
    let text = text.trim();
    if text.is_empty() {
        String::new()
    } else {
        format!("; last installer output: {text}")
    }
}

/// Installer entry point, from the environment or the development checkout beside this binary.
fn installer_path(environment: &dyn Fn(&str) -> Option<std::ffi::OsString>) -> Option<PathBuf> {
    if let Some(explicit) = environment("AIT_SERVER_OMP_NATIVE_HOST_INSTALLER") {
        let path = PathBuf::from(explicit);
        return path.is_file().then_some(path);
    }
    let executable = std::env::current_exe().ok()?;
    // target/debug/daemon -> repository root, where runtimes/ lives.
    let root = executable
        .parent()?
        .parent()?
        .parent()?
        .parent()
        .filter(|root| root.join("runtimes").is_dir())?;
    let installer = root.join(SOURCE_INSTALLER);
    installer.is_file().then_some(installer)
}

#[cfg(test)]
mod tests;
