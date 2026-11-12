//! Native Provider adapters owned by the independent server.

mod acp_config;
mod acp_permissions;
mod acp_streaming;
mod acp_transport;
#[cfg(test)]
mod agent_mcp_injection;
pub(crate) mod antigravity;
pub(crate) mod claude;
pub(crate) mod codex;
mod compaction;
pub mod configuration;
pub(crate) mod deepseek_harness;
pub mod diagnostics;
mod elicitation;
pub mod images;
pub mod metadata_process;
pub(crate) mod minimax;
mod notes;
pub(crate) mod opencode;
pub(crate) mod pi;
pub mod process_group;
mod prompt_blocks;
pub mod sandbox;
mod summary_model;
mod tool_detail;
/// Native usage parsing shared by adapters, including per-request ledger segments.
pub mod usage;

pub mod session_preview;
