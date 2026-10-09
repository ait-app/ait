//! Native Provider adapters owned by the independent server.

mod acp_transport;
pub(crate) mod antigravity;
pub(crate) mod claude;
pub(crate) mod codex;
mod configuration;
pub(crate) mod deepseek_harness;
mod elicitation;
mod images;
mod metadata_process;
mod notes;
pub(crate) mod opencode;
mod summary_model;
mod tool_detail;
mod usage;

mod session_preview;
