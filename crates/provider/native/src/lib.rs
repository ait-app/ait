//! Process, sandbox, ACP and media helpers shared by native provider adapters.

pub mod acp_config;
pub mod acp_permissions;
pub mod acp_session;
/// ACP session notifications projected into bounded timeline and progress events.
pub mod acp_streaming;
/// Owned, bounded JSON-RPC stdio connection to a native ACP process.
pub mod acp_transport;
pub mod auxiliary;
pub mod compaction;
pub mod configuration;
pub mod diagnostics;
pub mod elicitation;
pub mod goal;
pub mod images;
pub mod json_lines;
pub mod metadata_process;
pub mod notes;
pub mod process_group;
pub mod prompt_blocks;
/// Record and replay of a stdio JSON-lines native CLI for adapter tests.
#[cfg(all(unix, any(test, feature = "test-support")))]
pub mod replay;
pub mod sandbox;
pub mod session_preview;
pub mod side_runner;
pub mod summary_model;
pub mod todo;
pub mod tool_detail;
/// Native usage parsing shared by adapters, including per-request ledger segments.
pub mod usage;
