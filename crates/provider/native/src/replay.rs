//! Record and replay of a stdio JSON-lines native CLI for adapter tests.
//!
//! [`StdioPeer`] places `stdio_peer.py` where an adapter expects its native program. In record
//! mode the peer runs the real CLI behind a tee; in replay mode it plays a committed transcript
//! back and logs the first frame the adapter sends that the recording does not expect.

use std::collections::BTreeMap;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// The record/replay program, kept in the source tree so concurrent spawns never execute a file
/// that is still being written.
const PEER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/replay/stdio_peer.py");

/// Transcript format name written in the first line of every recording.
const FORMAT: &str = "ait-stdio-replay";

/// Placeholder for the session working directory, which every peer creates and substitutes.
const CWD: &str = "{{cwd}}";

/// A private stand-in for one native CLI with its own working directory.
#[derive(Debug)]
pub struct StdioPeer {
    root: tempfile::TempDir,
    program: PathBuf,
    cwd: String,
    transcript: PathBuf,
}

/// A recording of the real CLI: what to run and how to make its frames safe to commit.
#[derive(Debug, Clone, Copy)]
pub struct Recording<'a> {
    /// Real native CLI to run.
    pub program: &'a Path,
    /// `(placeholder, value)` pairs; every value is replaced by its placeholder in the recorded
    /// frames, as is [`StdioPeer::cwd`] by `{{cwd}}`.
    pub substitutions: &'a [(&'a str, &'a str)],
    /// Variables set for the real CLI only, such as a private home.
    pub environment: &'a [(&'a str, &'a str)],
    /// Object keys whose values are recorded as `"redacted"`, such as machine identifiers.
    pub redact: &'a [&'a str],
    /// JSON object describing the recording: provider, protocol, CLI version, scenario.
    pub meta: &'a Value,
}

impl StdioPeer {
    /// Replay `transcript` to the adapter that launches [`Self::program`].
    ///
    /// # Arguments
    ///
    /// * `name` - File name the adapter expects for its program, such as `codex`
    /// * `transcript` - Committed recording to play back
    /// * `substitutions` - `(placeholder, value)` pairs written into the recorded frames, in
    ///   addition to `{{cwd}}`, which is always [`Self::cwd`]
    ///
    /// # Panics
    ///
    /// Panics when the temporary root, the link or the configuration cannot be created.
    #[must_use]
    pub fn replay(name: &str, transcript: &Path, substitutions: &[(&str, &str)]) -> Self {
        Self::new(name, transcript, substitutions, None)
    }

    /// Run `recording.program` behind a tee and write its frames to `transcript`, replacing its
    /// contents; the first line is `recording.meta` plus the format header.
    ///
    /// # Arguments
    ///
    /// * `name` - File name the adapter expects for its program
    /// * `transcript` - Recording to write
    /// * `recording` - Real CLI, its environment and the scrubbing rules
    ///
    /// # Panics
    ///
    /// Panics when the transcript, the temporary root, the link or the configuration cannot be
    /// written, or when `recording.meta` is not a JSON object.
    #[must_use]
    pub fn record(name: &str, transcript: &Path, recording: &Recording<'_>) -> Self {
        let mut header = json!({"type": "meta", "format": FORMAT, "version": 1});
        header
            .as_object_mut()
            .expect("the header is an object")
            .extend(
                recording
                    .meta
                    .as_object()
                    .expect("meta is a JSON object")
                    .clone(),
            );
        std::fs::write(transcript, format!("{header}\n")).expect("start the transcript");
        Self::new(name, transcript, recording.substitutions, Some(recording))
    }

    fn new(
        name: &str,
        transcript: &Path,
        substitutions: &[(&str, &str)],
        recording: Option<&Recording<'_>>,
    ) -> Self {
        let root = tempfile::tempdir().expect("create the peer root");
        let program = root.path().join(name);
        symlink(PEER, &program).expect("link the stdio peer");
        let work = root.path().join("work");
        std::fs::create_dir(&work).expect("create the peer working directory");
        // The canonical path is what a native CLI reports back, so it is the one substituted.
        let cwd = std::fs::canonicalize(&work)
            .expect("resolve the peer working directory")
            .into_os_string()
            .into_string()
            .expect("the peer working directory is UTF-8");
        let mut placeholders: BTreeMap<&str, &str> = substitutions.iter().copied().collect();
        placeholders.insert(CWD, &cwd);
        let config = match recording {
            None => json!({
                "mode": "replay",
                "transcript": transcript,
                "substitutions": placeholders,
            }),
            Some(recording) => json!({
                "mode": "record",
                "transcript": transcript,
                "substitutions": placeholders,
                "program": recording.program,
                "env": recording.environment.iter().copied().collect::<BTreeMap<_, _>>(),
                "redact": recording.redact,
            }),
        };
        std::fs::write(root.path().join("replay.json"), config.to_string())
            .expect("write the peer configuration");
        Self {
            root,
            program,
            cwd,
            transcript: transcript.to_owned(),
        }
    }

    /// Path the adapter launches as its native program.
    #[must_use]
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// Canonical session working directory, substituted for `{{cwd}}`.
    #[must_use]
    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    /// The first frame the adapter sent that the recording did not expect, with the reason and
    /// both frames, or `None` while the replay matches.
    ///
    /// # Panics
    ///
    /// Panics when the progress log holds a line that is not JSON.
    #[must_use]
    pub fn mismatch(&self) -> Option<Value> {
        self.progress()
            .into_iter()
            .find(|entry| entry.get("mismatch").is_some())
    }

    /// Assert that every launch the recording holds happened and received every recorded
    /// adapter frame, and that the adapter sent nothing the recording does not expect.
    ///
    /// # Panics
    ///
    /// Panics with the first mismatch, or with the first launch that did not receive all of
    /// its recorded frames.
    pub fn assert_replayed(&self) {
        if let Some(mismatch) = self.mismatch() {
            panic!("the adapter diverged from the recording: {mismatch:#}");
        }
        let progress = self.progress();
        let recorded = std::fs::read_to_string(&self.transcript).expect("read the transcript");
        for spawn in recorded
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("transcript line is JSON"))
            .filter(|entry| entry["type"] == "spawn")
        {
            let process = &spawn["process"];
            let launch = progress.iter().filter(|entry| entry["process"] == *process);
            let finished = launch.clone().any(|entry| entry.get("finished").is_some());
            assert!(
                finished,
                "launch {process} received {} recorded adapter frames and then stopped",
                launch
                    .filter(|entry| entry.get("consumed").is_some())
                    .count()
            );
        }
    }

    fn progress(&self) -> Vec<Value> {
        std::fs::read_to_string(self.root.path().join("replay.log"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).expect("replay progress line is JSON"))
            .collect()
    }
}

#[cfg(test)]
mod tests;
