//! Ports that native adapters implement and the provider runtime drives.

pub mod account_usage;
pub mod agent_session;
pub mod bot_session;
pub mod controls;
pub mod diagnostics;
pub mod environment;
/// Native history discovery and inspection facts.
pub mod native_history;
/// Request usage that adapters report to the harness usage ledger.
pub mod usage_ledger;

/// Provider-independent auxiliary side conversations.
pub mod side_questions;
