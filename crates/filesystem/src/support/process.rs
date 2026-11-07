//! Bounded child processes shared by the Git and forge CLI adapters.
//!
//! Output is captured in anonymous temporary files so a chatty child never blocks on a full
//! pipe; the runner polls the child and the captured sizes, and on timeout, cancellation or
//! oversize output terminates the child's whole process group.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;

/// First wait before checking a running child: most local Git reads finish within it.
const POLL_START: Duration = Duration::from_millis(4);
/// Longest wait between checks, so slow network commands rarely wake the polling thread.
const POLL_MAX: Duration = Duration::from_millis(50);
/// Wait between checks while a terminated process group runs its signal handlers.
const TERMINATE_POLL: Duration = Duration::from_millis(10);
/// Time a terminated process group gets to run signal handlers, such as Git removing its
/// `index.lock`, before it is killed.
const TERMINATE_GRACE: Duration = Duration::from_millis(250);
/// Git subcommands that may open an SSH connection to a remote.
const NETWORK_SUBCOMMANDS: [&str; 5] = ["clone", "fetch", "ls-remote", "pull", "push"];
/// Fails on any passphrase, password or unknown host key instead of asking for it.
const BATCH_SSH_COMMAND: &str = "ssh -oBatchMode=yes";
/// Budget for reading local Git configuration before a network command.
const CONFIG_TIMEOUT: Duration = Duration::from_secs(10);
/// Stack buffer used to forward new standard-error bytes to a caller tap.
const TAP_BUFFER_BYTES: usize = 16 * 1024;

/// Callback receiving newly written standard-error bytes while a command runs.
pub(crate) type StderrTap<'a> = &'a mut dyn FnMut(&[u8]);

/// Lifetime and captured-output bounds of one child process.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Bounds {
    pub(crate) timeout: Duration,
    pub(crate) stdout_limit: u64,
    pub(crate) stderr_limit: u64,
}

/// Why a bounded child produced no usable output.
#[derive(Debug)]
pub(crate) enum RunError {
    /// The child could not be started.
    Spawn(std::io::Error),
    /// Capturing output or waiting for the child failed.
    Io(std::io::Error),
    /// The child outlived its deadline and was terminated.
    TimedOut,
    /// The caller cancelled the child, which was terminated.
    Cancelled,
    /// Standard output exceeded its limit.
    StdoutExceeded,
    /// Standard error exceeded its limit while the child was running.
    StderrExceeded,
}

/// Exit status and captured output of a completed child.
#[derive(Debug)]
pub(crate) struct Output {
    pub(crate) status: ExitStatus,
    pub(crate) stdout: Vec<u8>,
    /// Diagnostic output, truncated to its limit.
    pub(crate) stderr: Vec<u8>,
}

impl Output {
    /// Exit code, or -1 when a signal ended the child.
    pub(crate) fn code(&self) -> i32 {
        self.status.code().unwrap_or(-1)
    }
}

/// Git executable resolved once per process; see [`resolve_git_program`].
static GIT_PROGRAM: OnceLock<PathBuf> = OnceLock::new();

/// Create a `git` command in `cwd` that ignores the server's own `GIT_*` environment.
pub(crate) fn git_command(cwd: &Path) -> Command {
    let mut command = Command::new(git_program());
    command.current_dir(cwd);
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(name);
        }
    }
    command
}

/// Executable used to start Git, falling back to a `PATH` lookup if a resolved file disappears,
/// for example after a developer tools update.
fn git_program() -> &'static Path {
    let program = GIT_PROGRAM.get_or_init(resolve_git_program);
    if program.is_absolute() && !program.is_file() {
        return Path::new("git");
    }
    program
}

/// On macOS, `/usr/bin/git` is an `xcrun` shim that locates the developer toolchain on every
/// call, roughly doubling the cost of short reads; the toolchain's Git is resolved once instead.
/// Any other `PATH` entry, and every other platform, keeps the ordinary `git` lookup.
fn resolve_git_program() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        const SHIM: &str = "/usr/bin/git";
        let first = std::env::var_os("PATH").and_then(|path| {
            std::env::split_paths(&path)
                .map(|directory| directory.join("git"))
                .find(|candidate| candidate.is_file())
        });
        if first.as_deref() == Some(Path::new(SHIM))
            && let Some(resolved) = xcrun_git()
        {
            return resolved;
        }
    }
    PathBuf::from("git")
}

/// Toolchain Git reported by `xcrun`, when it names an existing file.
#[cfg(target_os = "macos")]
fn xcrun_git() -> Option<PathBuf> {
    let bounds = Bounds {
        timeout: CONFIG_TIMEOUT,
        stdout_limit: 4 * 1024,
        stderr_limit: 4 * 1024,
    };
    let mut command = Command::new("/usr/bin/xcrun");
    command.args(["--find", "git"]);
    let output = run_bounded(&mut command, bounds, None).ok()?;
    let path = PathBuf::from(String::from_utf8(output.stdout).ok()?.trim());
    (output.status.success() && path.is_absolute() && path.is_file()).then_some(path)
}

/// Make a [`git_command`] for `arguments` in `cwd` fail instead of prompting.
///
/// Children have no terminal and run in their own process group, so an SSH passphrase or
/// host-key prompt would stop the child until its timeout and an askpass helper could open a
/// dialog. Terminal, credential-manager and askpass prompts are disabled; for subcommands that
/// may reach a remote, ssh runs in batch mode unless the repository configures its own
/// `core.sshCommand`, which is honored. The server's own `GIT_SSH_COMMAND` is never inherited.
pub(crate) fn disable_git_prompts(command: &mut Command, cwd: &Path, arguments: &[&str]) {
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .env("SSH_ASKPASS_REQUIRE", "never");
    let network = arguments
        .first()
        .is_some_and(|subcommand| NETWORK_SUBCOMMANDS.contains(subcommand));
    if network && !configured_ssh_command(cwd) {
        command.env("GIT_SSH_COMMAND", BATCH_SSH_COMMAND);
    }
}

/// Whether the effective Git configuration in `cwd` sets `core.sshCommand`.
fn configured_ssh_command(cwd: &Path) -> bool {
    let bounds = Bounds {
        timeout: CONFIG_TIMEOUT,
        stdout_limit: 64 * 1024,
        stderr_limit: 64 * 1024,
    };
    let mut command = git_command(cwd);
    command.args(["config", "--get", "core.sshCommand"]);
    run_bounded(&mut command, bounds, None).is_ok_and(|output| output.status.success())
}

/// Run `command` with null stdin in its own process group within `bounds`.
///
/// # Errors
/// Returns [`RunError`] when the child cannot start, outlives `bounds.timeout`, is cancelled,
/// or writes more than a configured limit; the whole process group is then terminated.
pub(crate) fn run_bounded(
    command: &mut Command,
    bounds: Bounds,
    cancellation: Option<&CancellationToken>,
) -> Result<Output, RunError> {
    run_bounded_tapping(command, bounds, cancellation, None)
}

/// [`run_bounded`] that also forwards every new standard-error byte to `stderr_tap`.
///
/// The tap runs on the polling thread, so it must only do bounded synchronous work (for
/// example parsing Git progress lines). Reading uses positional I/O and never moves the
/// child's write offset.
///
/// # Errors
/// Returns the same failures as [`run_bounded`], plus standard-error read failures.
pub(crate) fn run_bounded_tapping(
    command: &mut Command,
    bounds: Bounds,
    cancellation: Option<&CancellationToken>,
    mut stderr_tap: Option<StderrTap<'_>>,
) -> Result<Output, RunError> {
    if cancellation.is_some_and(CancellationToken::is_cancelled) {
        return Err(RunError::Cancelled);
    }
    let mut stdout = tempfile::tempfile().map_err(RunError::Io)?;
    let mut stderr = tempfile::tempfile().map_err(RunError::Io)?;
    command
        .stdin(Stdio::null())
        .stdout(stdout.try_clone().map_err(RunError::Io)?)
        .stderr(stderr.try_clone().map_err(RunError::Io)?);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(RunError::Spawn)?;
    let deadline = Instant::now() + bounds.timeout;
    let mut tapped = 0_u64;
    let mut tap_buffer = [0_u8; TAP_BUFFER_BYTES];
    let mut poll = POLL_START;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                terminate(&mut child);
                return Err(RunError::Io(error));
            }
        }
        if let Some(tap) = stderr_tap.as_mut() {
            tap_stderr(&stderr, &mut tapped, &mut tap_buffer, &mut **tap)?;
        }
        let failure = if cancellation.is_some_and(CancellationToken::is_cancelled) {
            Some(RunError::Cancelled)
        } else if !within_limit(&stdout, bounds.stdout_limit) {
            Some(RunError::StdoutExceeded)
        } else if !within_limit(&stderr, bounds.stderr_limit) {
            Some(RunError::StderrExceeded)
        } else if Instant::now() >= deadline {
            Some(RunError::TimedOut)
        } else {
            None
        };
        if let Some(failure) = failure {
            terminate(&mut child);
            return Err(failure);
        }
        std::thread::sleep(poll);
        poll = poll.saturating_mul(2).min(POLL_MAX);
    };
    if let Some(tap) = stderr_tap.as_mut() {
        tap_stderr(&stderr, &mut tapped, &mut tap_buffer, &mut **tap)?;
    }
    let stdout = read_limited(&mut stdout, bounds.stdout_limit)?;
    if stdout.len() as u64 > bounds.stdout_limit {
        return Err(RunError::StdoutExceeded);
    }
    let mut stderr = read_limited(&mut stderr, bounds.stderr_limit)?;
    stderr.truncate(usize::try_from(bounds.stderr_limit).unwrap_or(usize::MAX));
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

/// Forward standard-error bytes written since `offset` and advance it.
fn tap_stderr(
    file: &File,
    offset: &mut u64,
    buffer: &mut [u8],
    tap: &mut dyn FnMut(&[u8]),
) -> Result<(), RunError> {
    let length = file.metadata().map_err(RunError::Io)?.len();
    while *offset < length {
        let read = read_at(file, buffer, *offset).map_err(RunError::Io)?;
        if read == 0 {
            break;
        }
        tap(&buffer[..read]);
        *offset += read as u64;
    }
    Ok(())
}

#[cfg(unix)]
fn read_at(file: &File, buffer: &mut [u8], offset: u64) -> std::io::Result<usize> {
    std::os::unix::fs::FileExt::read_at(file, buffer, offset)
}

#[cfg(windows)]
fn read_at(file: &File, buffer: &mut [u8], offset: u64) -> std::io::Result<usize> {
    std::os::windows::fs::FileExt::seek_read(file, buffer, offset)
}

#[cfg(not(any(unix, windows)))]
fn read_at(_file: &File, _buffer: &mut [u8], _offset: u64) -> std::io::Result<usize> {
    Ok(0)
}

fn within_limit(file: &File, limit: u64) -> bool {
    file.metadata()
        .is_ok_and(|metadata| metadata.len() <= limit)
}

// Reads at most one byte past `limit`, so callers can tell an exact fit from an overflow.
fn read_limited(file: &mut File, limit: u64) -> Result<Vec<u8>, RunError> {
    file.seek(SeekFrom::Start(0)).map_err(RunError::Io)?;
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(RunError::Io)?;
    Ok(bytes)
}

/// Terminate `child`'s process group: SIGTERM, a short grace period, then SIGKILL.
///
/// Signalling the group also stops hooks, credential helpers and remote helpers the child
/// started, so none of them outlive the deadline or keep repository locks.
pub(crate) fn terminate(child: &mut Child) {
    #[cfg(unix)]
    {
        let group = format!("-{}", child.id());
        signal_group("-TERM", &group);
        let deadline = Instant::now() + TERMINATE_GRACE;
        while Instant::now() < deadline && matches!(child.try_wait(), Ok(None)) {
            std::thread::sleep(TERMINATE_POLL);
        }
        signal_group("-KILL", &group);
    }
    #[cfg(windows)]
    let _ = Command::new("taskkill")
        .args(["/PID", &child.id().to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(unix)]
fn signal_group(signal: &str, group: &str) {
    let _ = Command::new("/bin/kill")
        .args([signal, "--", group])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(test)]
mod tests;
