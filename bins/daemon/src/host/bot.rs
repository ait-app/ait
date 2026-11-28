use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use bot::mcp::Scopes;
use bot::ports::Credentials;
use bot::service::{Bots, Dependencies};
use domain::ChannelId;
use model::ErrorCode;
use model::workspace::registry::{ProjectRegistry, WorkspaceRegistry};
use model::workspace::worktrees::WorktreeProvisioning;
use persistence::storage::bot::{FileStore, SecretFiles};
use provider::service::agent_execution::AgentExecution;
use secrecy::{ExposeSecret, SecretString};

/// Daemon-injected environment references and the daemon's own per-channel secret files.
#[derive(Debug)]
struct HostCredentials {
    secrets: SecretFiles,
}

/// Longest credential value accepted from any host source.
const CREDENTIAL_LIMIT: usize = 4096;

fn checked_credential(value: String) -> Result<SecretString, ErrorCode> {
    if value.is_empty() || value.len() > CREDENTIAL_LIMIT || value.chars().any(char::is_control) {
        return Err(ErrorCode::BotIo);
    }
    Ok(SecretString::from(value))
}

/// Longest multi-line key accepted from the environment; a 4096-bit PEM key is about 3.3 KiB.
const KEY_LIMIT: usize = 16 * 1024;

/// A multi-line key such as a PEM private key: line breaks are its format, other control
/// characters are still refused.
fn checked_key(value: String) -> Result<SecretString, ErrorCode> {
    let line_break = |c: char| c == '\n' || c == '\r';
    if value.trim().is_empty()
        || value.len() > KEY_LIMIT
        || value.chars().any(|c| c.is_control() && !line_break(c))
    {
        return Err(ErrorCode::BotIo);
    }
    Ok(SecretString::from(value))
}

/// The value of the environment variable an `env:AIT_SERVER_CREDENTIAL_*` reference names.
fn environment_value(reference: &str) -> Result<String, ErrorCode> {
    if !bot::ports::valid_credential_reference(reference) {
        return Err(ErrorCode::InvalidMessage);
    }
    let name = reference
        .strip_prefix("env:")
        .ok_or(ErrorCode::InvalidMessage)?;
    std::env::var(name).map_err(|_| ErrorCode::BotIo)
}

impl Credentials for HostCredentials {
    fn store(&self, channel: ChannelId, secret: &SecretString) -> Result<(), ErrorCode> {
        self.secrets
            .save(channel, secret.expose_secret())
            .map_err(|_| ErrorCode::BotIo)
    }

    fn delete(&self, channel: ChannelId) -> Result<(), ErrorCode> {
        self.secrets.delete(channel).map_err(|_| ErrorCode::BotIo)
    }

    fn resolve(&self, reference: &str) -> Result<SecretString, ErrorCode> {
        if let Some(channel) = bot::ports::stored_credential_channel(reference) {
            let value = self.secrets.load(channel).map_err(|_| ErrorCode::BotIo)?;
            return checked_credential(value);
        }
        checked_credential(environment_value(reference)?)
    }

    fn resolve_key(&self, reference: &str) -> Result<SecretString, ErrorCode> {
        if bot::ports::stored_credential_channel(reference).is_some() {
            return self.resolve(reference);
        }
        checked_key(environment_value(reference)?)
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
    scopes: Arc<Scopes>,
    worktrees: Arc<dyn WorktreeProvisioning>,
) -> anyhow::Result<Bots> {
    let root = data.join("workspace-bots");
    let bots = Bots::spawn(Dependencies {
        store: Arc::new(FileStore::new(root.clone())),
        workspaces: Arc::new(workspaces.clone()),
        projects: Arc::new(projects.clone()),
        executor: Arc::new(execution.clone()),
        credentials: Arc::new(HostCredentials {
            secrets: SecretFiles::new(root.join("credentials")),
        }),
        scopes,
        image_directory: Some(data.join("agents/provider-images")),
        worktrees,
    })
    .context("start Bot service")?;
    if let Ok(runtime) = tokio::runtime::Handle::try_current() {
        let legacy = data.join("bots");
        runtime.spawn(async move {
            bot::legacy::discard(&legacy, &execution).await;
        });
    } else {
        tracing::warn!("no async runtime; skipped discarding the legacy Bot store");
    }
    Ok(bots)
}

#[cfg(test)]
mod tests;
