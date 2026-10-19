//! Ephemeral account identity and a lazy quota read from the same authentication context.

use std::fmt;

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::agent_session::AgentSessionFuture;

/// Opaque authentication identity, compared only in memory and never serialized or logged.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct UsageIdentity([u8; 32]);

impl UsageIdentity {
    /// Hash provider and native identity material without retaining its plaintext.
    pub(crate) fn from_native(provider: &str, material: &[u8]) -> Self {
        let mut digest = Sha256::new();
        digest.update(b"ait-account-usage-v1");
        digest.update(provider.len().to_le_bytes());
        digest.update(provider.as_bytes());
        digest.update(material.len().to_le_bytes());
        digest.update(material);
        Self(digest.finalize().into())
    }
}

impl fmt::Debug for UsageIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("UsageIdentity(<redacted>)")
    }
}

/// Authentication is resolved before cache lookup; polling happens only when needed.
pub(crate) struct PreparedUsage<'a> {
    /// Known native account identity; absence disables historical fallback and cache reuse.
    pub(crate) identity: Option<UsageIdentity>,
    /// Quota operation bound to the authentication context used for `identity`.
    pub(crate) report: AgentSessionFuture<'a, Value>,
}

impl fmt::Debug for PreparedUsage<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedUsage")
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
