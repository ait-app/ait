//! Workspace-owned conversational Bots with durable, isolated multi-channel execution.

pub mod capabilities;
mod channels;
pub mod dispatch;
mod execution;
pub mod ports;
mod protocol;
pub mod service;
mod state;
#[cfg(test)]
mod test_support;
