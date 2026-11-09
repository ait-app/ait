//! Host-supplied secrets and native-execution ports.

use domain::ChannelId;
use domain::bot::SessionOwner;
use model::ErrorCode;
use provider::service::agent_execution::AgentExecution;
use provider::service::bot_execution::Action;
use secrecy::SecretString;
use serde_json::Value;

/// Prefix of a reference to a secret the daemon stores for one channel. It is deliberately a
/// valid environment reference, so a downgraded daemon reports the channel as unavailable
/// instead of refusing the whole catalog at startup.
const STORED_REFERENCE_PREFIX: &str = "env:AIT_SERVER_CREDENTIAL_STORED_";

/// Host secret access; implementations must never log secret values.
pub trait Credentials: std::fmt::Debug + Send + Sync {
    /// Resolve an `env:AIT_SERVER_CREDENTIAL_*` or stored channel reference at runtime.
    /// # Errors
    /// Returns a safe unavailable-credential error.
    fn resolve(&self, reference: &str) -> Result<SecretString, ErrorCode>;

    /// Privately persist `secret` for `channel`, replacing any earlier value.
    /// # Errors
    /// Returns a safe error; hosts without writable secret storage always fail.
    fn store(&self, _channel: ChannelId, _secret: &SecretString) -> Result<(), ErrorCode> {
        Err(ErrorCode::BotIo)
    }

    /// Remove any secret stored for `channel`; an absent secret is not an error.
    /// # Errors
    /// Returns a safe error when an existing secret cannot be removed.
    fn delete(&self, _channel: ChannelId) -> Result<(), ErrorCode> {
        Ok(())
    }
}

/// The reference a channel uses for the secret the daemon stores for it.
pub(crate) fn stored_credential_reference(channel: ChannelId) -> String {
    let name = channel.to_string().replace('-', "").to_ascii_uppercase();
    format!("{STORED_REFERENCE_PREFIX}{name}")
}

/// The channel named by a stored-secret reference, or `None` for any other reference.
#[must_use]
pub fn stored_credential_channel(reference: &str) -> Option<ChannelId> {
    reference
        .strip_prefix(STORED_REFERENCE_PREFIX)?
        .parse()
        .ok()
}

/// Validate a nonsecret reference to a daemon-injected credential environment variable.
#[must_use]
pub fn valid_credential_reference(reference: &str) -> bool {
    reference
        .strip_prefix("env:AIT_SERVER_CREDENTIAL_")
        .is_some_and(|name| {
            !name.is_empty()
                && name.len() <= 96
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        })
}

/// Owner-checked native execution, injectable for offline Bot tests.
#[async_trait::async_trait]
pub trait Executor: std::fmt::Debug + Send + Sync {
    /// Execute one operation without moving session ownership to a client connection.
    /// # Errors
    /// Returns validation, inactive workspace, native or persistence failures.
    async fn execute(&self, owner: SessionOwner, action: Action) -> Result<Value, ErrorCode>;
}

#[async_trait::async_trait]
impl Executor for AgentExecution {
    async fn execute(&self, owner: SessionOwner, action: Action) -> Result<Value, ErrorCode> {
        self.bot(&owner, action).await
    }
}
