//! Shared change-request checkout input used by Workspace and Agent creation.

use serde::Deserialize;

/// A forge change request selected as a checkout source.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeRequestCheckoutSource {
    /// Discriminator retained from Paseo's schema.
    kind: ChangeRequestCheckoutKind,
    /// Optional forge identifier.
    #[serde(default)]
    forge: Option<String>,
    /// Positive change-request number.
    #[serde(deserialize_with = "positive_number")]
    number: u64,
    /// Optional forge project path.
    #[serde(default)]
    project_path: Option<String>,
}

/// Checkout-source discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ChangeRequestCheckoutKind {
    /// A pull or merge request.
    ChangeRequest,
}

impl ChangeRequestCheckoutSource {
    /// Translate validated wire fields to the shared provisioning intent.
    #[must_use]
    pub fn into_intent(self) -> crate::workspace::worktrees::WorktreeChangeRequest {
        crate::workspace::worktrees::WorktreeChangeRequest {
            forge: self.forge,
            number: self.number,
            project_path: self.project_path,
        }
    }
}

fn positive_number<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    let number = u64::deserialize(deserializer)?;
    if number == 0 || number > 9_007_199_254_740_991 {
        return Err(serde::de::Error::custom(
            "expected a positive JavaScript-safe integer",
        ));
    }
    Ok(number)
}

#[cfg(test)]
mod tests;
