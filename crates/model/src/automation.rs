//! Hub automation: the center drives Agents on this Host for chat messages
//! (Feishu, Slack, Telegram) through its own Hub sessions.
//!
//! The Host decides. [`Settings`] stay local; the daemon publishes them with
//! [`PUBLISH_METHOD`], and the center opens automation sessions only to a serve
//! link that published `enabled: true`. The local API admits such a session only
//! while the settings are enabled and confines it to the automation policy.

use serde::{Deserialize, Serialize};

/// Center method that publishes this Host's automation settings.
pub const PUBLISH_METHOD: &str = "hub.automation.publish";

/// `labels.origin` of every Agent an automation session creates.
pub const ORIGIN_LABEL: &str = "origin";

/// Value of [`ORIGIN_LABEL`] marking an automation Agent.
pub const ORIGIN: &str = "automation";

/// Settings name at most this many directories, matching the center.
pub const MAX_ROOTS: usize = 64;

/// Settings name at most this many providers, matching the center.
pub const MAX_PROVIDERS: usize = 32;

/// One directory automation may run Agents in, with its subdirectories.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Root {
    /// Absolute path.
    pub path: String,
    /// Optional name shown to the account owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// The Host's automation settings; off by default.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// Whether the center may open automation sessions to this Host.
    #[serde(default)]
    pub enabled: bool,
    /// Directories automation may use; none admits nothing.
    #[serde(default)]
    pub roots: Vec<Root>,
    /// Providers automation may use; empty admits every provider.
    #[serde(default)]
    pub providers: Vec<String>,
}

impl Settings {
    /// Whether the settings stay within the limits the center accepts.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        let path = |path: &str| {
            path.starts_with('/') && path.len() <= 4096 && !path.chars().any(char::is_control)
        };
        let label = |label: &str| {
            !label.is_empty()
                && label.chars().count() <= 200
                && !label.chars().any(char::is_control)
        };
        let provider = |name: &str| {
            !name.is_empty()
                && name.len() <= 64
                && name.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'-' | b'_' | b'.')
                })
        };
        self.roots.len() <= MAX_ROOTS
            && self.providers.len() <= MAX_PROVIDERS
            && self
                .roots
                .iter()
                .all(|root| path(&root.path) && root.label.as_deref().is_none_or(label))
            && self.providers.iter().all(|name| provider(name))
    }

    /// Whether `provider` may run here.
    #[must_use]
    pub fn admits_provider(&self, provider: &str) -> bool {
        self.providers.is_empty() || self.providers.iter().any(|name| name == provider)
    }

    /// The publication body for [`PUBLISH_METHOD`].
    #[must_use]
    pub fn publication(&self) -> serde_json::Value {
        serde_json::json!({
            "version": 1,
            "enabled": self.enabled,
            "roots": self.roots,
            "providers": self.providers,
        })
    }
}

#[cfg(test)]
mod tests;
