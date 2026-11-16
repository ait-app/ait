//! `OpenCode` official ACP adapter. Native authentication, tools and storage remain `OpenCode` owned.

mod client;
mod config;
mod discovery;
mod history;
mod interactions;
mod launcher;
mod observation;
mod session;
mod side;
mod streaming;
mod subagents;
mod summary;
mod tool;

pub use client::OpenCodeClient;

const PROVIDER: &str = "opencode";

#[cfg(test)]
mod tests;
