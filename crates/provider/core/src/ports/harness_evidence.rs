//! Where the host's incident collector finds an adapter's native program and logs (ADR-109).

/// Native program and log locations an adapter contributes to local incident evidence.
///
/// The collector resolves these from the daemon environment without I/O, then runs the program
/// with `--version` and reads bounded, timestamped log excerpts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HarnessEvidenceSource {
    /// Environment variable that overrides the native program, such as `AIT_SERVER_CODEX_BIN`.
    pub program_variable: &'static str,
    /// Program run when `program_variable` is unset, resolved through `PATH`.
    pub default_program: &'static str,
    /// Environment variable naming a dedicated native log directory; it takes precedence over
    /// `native_logs`.
    pub log_directory_variable: &'static str,
    /// Where native logs are found when `log_directory_variable` is unset.
    pub native_logs: NativeLogs,
}

/// Default location of an adapter's native log directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeLogs {
    /// No default location is known.
    Unknown,
    /// `<root>/<subdirectory>`, where the root is `root_variable` when set, otherwise
    /// `home_directory` under the user's home directory.
    UnderRoot {
        /// Environment variable that relocates the native configuration root.
        root_variable: &'static str,
        /// Root directory relative to the user's home directory.
        home_directory: &'static str,
        /// Log directory inside the root.
        subdirectory: &'static str,
    },
    /// The native program prints its absolute log directory when run with these arguments.
    ReportedByProgram(&'static [&'static str]),
}
