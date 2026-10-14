//! Ephemeral per-Agent process environment; never part of persisted Agent configuration.

use std::collections::BTreeMap;

use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

use super::agent_session::AgentSessionError;

/// Bounded environment overrides whose values are redacted from debug output.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(try_from = "BTreeMap<String, String>")]
pub(crate) struct AgentEnvironment(BTreeMap<String, SecretString>);

impl TryFrom<BTreeMap<String, String>> for AgentEnvironment {
    type Error = AgentSessionError;

    fn try_from(values: BTreeMap<String, String>) -> Result<Self, Self::Error> {
        if values.len() > 256
            || values.iter().any(|(key, value)| {
                key.is_empty()
                    || key.len() > 256
                    || key.contains(['=', '\0'])
                    || value.len() > 65_536
                    || value.contains('\0')
            })
            || values
                .iter()
                .map(|(key, value)| key.len() + value.len())
                .sum::<usize>()
                > 262_144
        {
            return Err(AgentSessionError::Rejected);
        }
        Ok(Self(
            values
                .into_iter()
                .map(|(key, value)| (key, value.into()))
                .collect(),
        ))
    }
}

impl AgentEnvironment {
    /// Whether native launch requires no explicit environment overrides.
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Expose values only while configuring a child process; callers must not log them.
    pub(crate) fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0
            .iter()
            .map(|(key, value)| (key.as_str(), value.expose_secret()))
    }
}

#[cfg(test)]
mod tests;
