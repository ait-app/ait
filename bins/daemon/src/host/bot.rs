use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use bot::ports::Credentials;
use bot::service::{Bots, Dependencies};
use model::ErrorCode;
use model::workspace::registry::{ProjectRegistry, WorkspaceRegistry};
use persistence::storage::bot::FileStore;
use provider::service::agent_execution::AgentExecution;
use secrecy::SecretString;

#[derive(Debug)]
struct Environment;

impl Credentials for Environment {
    fn resolve(&self, reference: &str) -> Result<SecretString, ErrorCode> {
        if !bot::ports::valid_credential_reference(reference) {
            return Err(ErrorCode::InvalidMessage);
        }
        let name = reference
            .strip_prefix("env:")
            .ok_or(ErrorCode::InvalidMessage)?;
        let value = std::env::var(name).map_err(|_| ErrorCode::BotIo)?;
        if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
            return Err(ErrorCode::BotIo);
        }
        Ok(SecretString::from(value))
    }
}

pub(super) fn compose<
    W: WorkspaceRegistry + Clone + 'static,
    P: ProjectRegistry + Clone + 'static,
>(
    data: &Path,
    workspaces: &W,
    projects: &P,
    execution: AgentExecution,
) -> anyhow::Result<Bots> {
    Bots::spawn(Dependencies {
        store: Arc::new(FileStore::new(data.join("bots"))),
        workspaces: Arc::new(workspaces.clone()),
        projects: Arc::new(projects.clone()),
        executor: Arc::new(execution),
        credentials: Arc::new(Environment),
    })
    .context("start Bot service")
}

#[cfg(test)]
mod tests;
