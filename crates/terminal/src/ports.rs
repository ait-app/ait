//! Native process boundary; the terminal service does not construct operating-system resources.

use std::collections::BTreeMap;
use std::fmt::Debug;

use crate::Error;
use crate::protocol::{Input, Restore, Size};

/// Validated process launch parameters.
#[derive(Clone)]
pub struct Launch {
    /// Canonical working directory.
    pub(crate) cwd: String,
    /// Optional executable; defaults to the host shell.
    pub(crate) command: Option<String>,
    /// Literal executable arguments.
    pub(crate) args: Vec<String>,
    /// Initial dimensions.
    pub(crate) size: Size,
    /// Workspace-specific environment overrides.
    pub(crate) env: BTreeMap<String, String>,
}

impl Debug for Launch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Launch")
            .field("cwd", &self.cwd)
            .field("command", &self.command)
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

/// Atomic screen/output observation at one revision.
#[derive(Debug, Clone)]
pub struct Observation {
    /// Cursor to use on the next observation.
    pub(crate) revision: u64,
    /// Current screen dimensions.
    pub(crate) size: Size,
    /// Ordered binary payloads without their connection-specific slot headers.
    pub(crate) frames: Vec<(crate::protocol::Opcode, Vec<u8>)>,
    /// The reader has drained and the process has exited.
    pub exited: bool,
}

/// PTY process operations; callers serialize access per process.
pub trait Process: Debug + Send {
    /// Read title updates without retaining process output in metadata.
    fn title(&self) -> Option<String>;
    /// Whether the process has exited and output has drained.
    ///
    /// # Errors
    /// Returns a native process inspection error.
    fn exited(&mut self) -> Result<bool, Error>;
    /// Queue input or apply a validated size change.
    ///
    /// # Errors
    /// Returns invalid input, a full input queue, or PTY failure.
    fn send(&mut self, input: &Input) -> Result<(), Error>;
    /// Atomically restore or read output after `revision`. `None` begins a subscription.
    ///
    /// # Errors
    /// Returns process inspection or snapshot serialization failures.
    fn observe(
        &mut self,
        revision: Option<u64>,
        restore: Option<&Restore>,
    ) -> Result<Observation, Error>;
    /// Read all retained history plus visible rows as plain text.
    ///
    /// # Errors
    /// Returns a screen access failure.
    fn capture(&self) -> Result<Vec<String>, Error>;
    /// Kill and reap the child, including its PTY process group where supported.
    ///
    /// # Errors
    /// Returns termination or reaping failures.
    fn kill(&mut self) -> Result<(), Error>;
}

/// Factory boundary for creating PTY processes and validating working directories.
pub trait Runtime: Debug + Send + Sync {
    /// Resolve an absolute directory; reject missing/non-directory paths.
    ///
    /// # Errors
    /// Returns invalid path or filesystem errors.
    fn directory(&self, path: &str) -> Result<String, Error>;
    /// Start one process; failures must release partially constructed resources.
    ///
    /// # Errors
    /// Returns invalid dimensions, spawn errors, or exhausted resources.
    fn spawn(&self, launch: &Launch) -> Result<Box<dyn Process>, Error>;
}
