//! Ephemeral per-Agent process environment; never part of persisted Agent configuration.

use std::collections::BTreeMap;

use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

use super::agent_session::AgentSessionError;

/// Bounded environment overrides whose values are redacted from debug output.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(try_from = "BTreeMap<String, String>")]
pub struct AgentEnvironment(BTreeMap<String, SecretString>);

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

    /// Resolve `name` as the native process will see it: overrides first, then the daemon's.
    ///
    /// Callers must not log the returned value.
    #[must_use]
    pub fn variable(&self, name: &str) -> Option<String> {
        self.0.get(name).map_or_else(
            || std::env::var(name).ok(),
            |value| Some(value.expose_secret().to_owned()),
        )
    }

    /// Expose values only at native integration boundaries; callers must not log them.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0
            .iter()
            .map(|(key, value)| (key.as_str(), value.expose_secret()))
    }
}

#[cfg(test)]
mod tests;
