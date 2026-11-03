//! Host-supplied secrets and native-execution ports.

use domain::bot::SessionOwner;
use model::ErrorCode;
use provider::service::agent_execution::AgentExecution;
use provider::service::bot_execution::Action;
use secrecy::SecretString;
use serde_json::Value;

/// Resolve only environment references; implementations must never persist or log secret values.
pub trait Credentials: std::fmt::Debug + Send + Sync {
    /// Resolve a validated `env:AIT_SERVER_CREDENTIAL_*` reference at runtime.
    /// # Errors
    /// Returns a safe unavailable-credential error.
    fn resolve(&self, reference: &str) -> Result<SecretString, ErrorCode>;
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
