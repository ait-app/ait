//! Script run records: checkout evidence, bounded output capture and retained history.

use std::collections::{HashMap, VecDeque};
use std::io::{PipeReader, Read};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use tokio::sync::watch;

use super::lock;
use crate::automation::ports::{
    ScriptRunCompletion, ScriptRunOutcome, ScriptRunSnapshot, WorkspaceAutomationError,
};
use crate::support::time::now_timestamp;

/// Leading output bytes kept for every run; the command echo and early failures live here.
const HEAD_BYTES: usize = 16 * 1024;
/// Trailing output bytes kept for every run; test summaries and final errors live here.
const TAIL_BYTES: usize = 48 * 1024;
/// Separator inserted where bytes between the head and the tail were dropped.
const TRUNCATION_MARKER: &[u8] = b"\n... script output truncated ...\n";
/// Finished runs retained per Workspace in addition to each script's current run.
const RUN_HISTORY: usize = 16;
/// Largest number of caller arguments accepted for one run.
const MAX_ARGS: usize = 64;
/// Largest single caller argument in bytes.
const MAX_ARG_BYTES: usize = 4096;
/// Read size of the output pipe.
const READ_CHUNK: usize = 8 * 1024;

/// Combined stdout and stderr, bounded to its first and last bytes.
#[derive(Debug, Default)]
pub(super) struct OutputBuffer {
    head: Vec<u8>,
    tail: VecDeque<u8>,
    total: u64,
}

impl OutputBuffer {
    /// Append `bytes`, keeping at most `HEAD_BYTES` leading and `TAIL_BYTES` trailing bytes.
    pub(super) fn push(&mut self, mut bytes: &[u8]) {
        self.total = self
            .total
            .saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        if self.head.len() < HEAD_BYTES {
            let taken = bytes.len().min(HEAD_BYTES - self.head.len());
            self.head.extend_from_slice(&bytes[..taken]);
            bytes = &bytes[taken..];
        }
        if bytes.len() >= TAIL_BYTES {
            self.tail.clear();
            self.tail.extend(&bytes[bytes.len() - TAIL_BYTES..]);
            return;
        }
        let overflow = (self.tail.len() + bytes.len()).saturating_sub(TAIL_BYTES);
        self.tail.drain(..overflow);
        self.tail.extend(bytes);
    }

    /// Render the retained bytes as lossy UTF-8 with the total written and truncation flag.
    pub(super) fn render(&self) -> (String, u64, bool) {
        let kept = self.head.len() + self.tail.len();
        let truncated = self.total > u64::try_from(kept).unwrap_or(u64::MAX);
        let mut bytes = Vec::with_capacity(kept + TRUNCATION_MARKER.len());
        bytes.extend_from_slice(&self.head);
        if truncated {
            bytes.extend_from_slice(TRUNCATION_MARKER);
        }
        let (front, back) = self.tail.as_slices();
        bytes.extend_from_slice(front);
        bytes.extend_from_slice(back);
        (
            String::from_utf8_lossy(&bytes).into_owned(),
            self.total,
            truncated,
        )
    }
}

/// Checkout state recorded when a run starts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct CheckoutEvidence {
    commit: Option<String>,
    dirty: Option<bool>,
}

/// Read `HEAD` and the presence of uncommitted or untracked changes below `cwd`.
/// Non-Git directories and Git failures leave both fields unknown.
pub(super) fn checkout_evidence(cwd: &str) -> CheckoutEvidence {
    let commit = git_stdout(cwd, &["rev-parse", "--verify", "HEAD"])
        .map(|output| output.trim().to_owned())
        .filter(|commit| !commit.is_empty());
    let dirty = commit
        .is_some()
        .then(|| git_stdout(cwd, &["status", "--porcelain"]))
        .flatten()
        .map(|status| !status.trim().is_empty());
    CheckoutEvidence { commit, dirty }
}

fn git_stdout(cwd: &str, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        // Evidence reads must not contend with the Agent's own index writes.
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Refuse argument lists that exceed the bounds or cannot be passed to a process.
///
/// # Errors
/// Returns `InvalidArguments` for too many, oversized or NUL-containing arguments, and for any
/// argument on platforms without a POSIX shell.
pub(super) fn validate_args(args: &[String]) -> Result<(), WorkspaceAutomationError> {
    if !cfg!(unix) && !args.is_empty() {
        // `cmd` re-parses its whole command line, so arguments cannot be passed safely.
        return Err(WorkspaceAutomationError::InvalidArguments(
            "Script arguments require a POSIX shell".to_owned(),
        ));
    }
    if args.len() > MAX_ARGS {
        return Err(WorkspaceAutomationError::InvalidArguments(format!(
            "Scripts accept at most {MAX_ARGS} arguments"
        )));
    }
    if args.iter().any(|arg| arg.len() > MAX_ARG_BYTES) {
        return Err(WorkspaceAutomationError::InvalidArguments(format!(
            "Script arguments are limited to {MAX_ARG_BYTES} bytes each"
        )));
    }
    if args.iter().any(|arg| arg.contains('\0')) {
        return Err(WorkspaceAutomationError::InvalidArguments(
            "Script arguments cannot contain NUL bytes".to_owned(),
        ));
    }
    Ok(())
}

/// Drain `reader` into `output` on a dedicated thread until every writer has closed.
///
/// # Errors
/// Returns thread creation failures; the caller must then stop the writing child.
pub(super) fn capture_output(
    mut reader: PipeReader,
    output: Arc<Mutex<OutputBuffer>>,
    script_name: &str,
) -> std::io::Result<()> {
    thread::Builder::new()
        .name(format!("workspace-script-output-{script_name}"))
        .spawn(move || {
            let mut chunk = [0_u8; READ_CHUNK];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) => return,
                    Ok(read) => lock(&output).push(&chunk[..read]),
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(error) => {
                        tracing::warn!(%error, "could not read Workspace script output");
                        return;
                    }
                }
            }
        })
        .map(drop)
}

/// One started script run and its completion signal.
#[derive(Debug)]
pub(super) struct ScriptRun {
    run_id: String,
    script_name: String,
    args: Vec<String>,
    evidence: CheckoutEvidence,
    started_at: String,
    started: Instant,
    finished_at: Option<String>,
    duration_ms: Option<u64>,
    outcome: ScriptRunOutcome,
    exit_code: Option<i32>,
    output: Arc<Mutex<OutputBuffer>>,
    completion: watch::Sender<bool>,
}

impl ScriptRun {
    /// Record a run that has just started writing to `output`.
    pub(super) fn start(
        script_name: &str,
        args: &[String],
        evidence: CheckoutEvidence,
        output: Arc<Mutex<OutputBuffer>>,
    ) -> Self {
        Self {
            run_id: format!("run_{}", uuid::Uuid::new_v4().simple()),
            script_name: script_name.to_owned(),
            args: args.to_vec(),
            evidence,
            started_at: now_timestamp(),
            started: Instant::now(),
            finished_at: None,
            duration_ms: None,
            outcome: ScriptRunOutcome::Running,
            exit_code: None,
            output,
            completion: watch::Sender::new(false),
        }
    }

    /// Daemon-assigned run identity.
    pub(super) fn id(&self) -> &str {
        &self.run_id
    }

    /// Last exit code, or none while running or after a signal.
    pub(super) const fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    /// Record completion once and release every waiter.
    pub(super) fn finish(&mut self, exit_code: Option<i32>, outcome: ScriptRunOutcome) {
        if self.outcome != ScriptRunOutcome::Running {
            return;
        }
        self.finished_at = Some(now_timestamp());
        self.duration_ms =
            Some(u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX));
        self.outcome = outcome;
        self.exit_code = exit_code;
        self.completion.send_replace(true);
    }

    /// Observe completion of this run.
    pub(super) fn completion(&self) -> ScriptRunCompletion {
        ScriptRunCompletion::new(self.completion.subscribe())
    }

    /// Project the run with its currently retained output.
    pub(super) fn snapshot(&self) -> ScriptRunSnapshot {
        let (output, output_bytes, truncated) = lock(&self.output).render();
        ScriptRunSnapshot {
            run_id: self.run_id.clone(),
            script_name: self.script_name.clone(),
            args: self.args.clone(),
            commit: self.evidence.commit.clone(),
            dirty: self.evidence.dirty,
            started_at: self.started_at.clone(),
            finished_at: self.finished_at.clone(),
            duration_ms: self.duration_ms,
            outcome: self.outcome,
            exit_code: self.exit_code,
            output,
            output_bytes,
            truncated,
        }
    }
}

/// Keep a replaced finished run, dropping the Workspace's oldest beyond `RUN_HISTORY`.
pub(super) fn retain(
    history: &mut HashMap<String, VecDeque<ScriptRun>>,
    workspace_id: &str,
    run: ScriptRun,
) {
    let runs = history.entry(workspace_id.to_owned()).or_default();
    runs.push_back(run);
    while runs.len() > RUN_HISTORY {
        runs.pop_front();
    }
}

#[cfg(test)]
mod tests;
