//! Non-secret Agent configuration and explicit catalog-default DTOs.

use serde::{Deserialize, Serialize};

/// Complete preset configuration. Unknown fields, including raw credential fields, are rejected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    /// Non-secret display name, 1–255 UTF-8 bytes, excluding controls or all-whitespace names.
    pub(crate) name: String,
    /// Configuration schema; currently only `codex`.
    pub(crate) driver_type: String,
    /// Explicit model identifier, 1–128 restricted ASCII bytes; not discovered or validated remotely.
    pub(crate) model: String,
    /// Optional `env:AIT_SERVER_CREDENTIAL_<NAME>` reference; never a credential value.
    #[serde(default)]
    pub(crate) credential_ref: Option<String>,
    /// Eligibility for selection; does not imply an executable adapter is installed.
    pub(crate) enabled: bool,
}

/// Create a preset or append an immutable full replacement to an existing preset.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Configure {
    /// Absent for creation; supply together with `expected_revision` for replacement.
    #[serde(default)]
    pub(crate) agent_id: Option<String>,
    /// Observed head revision; absent for creation, positive for replacement.
    #[serde(default)]
    pub(crate) expected_revision: Option<u64>,
    /// Complete replacement configuration.
    pub(crate) config: Config,
    /// Method-scoped durable retry key, 1–128 visible ASCII bytes.
    pub(crate) idempotency_key: String,
}

/// Read a current or historical immutable configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Get {
    /// Stable Agent UUID.
    pub(crate) agent_id: String,
    /// Absent for the current head, otherwise the exact positive revision.
    #[serde(default)]
    pub(crate) revision: Option<u64>,
}

/// Stable-ID keyset pagination of current configurations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct List {
    /// Exclusive Agent UUID cursor.
    #[serde(default)]
    pub(crate) after: Option<String>,
    /// Between 1 and 50; defaults to 20.
    #[serde(default = "default_limit")]
    pub(crate) limit: usize,
}

fn default_limit() -> usize {
    20
}

/// Empty parameters for reading the explicit catalog default.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GetDefault {}

/// Replace the explicit default; every field is required, including a null clear target.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SetDefault {
    /// Agent UUID or explicit null to clear; omission is rejected.
    #[serde(deserialize_with = "required_optional")]
    pub(crate) agent_id: Option<String>,
    /// Last observed default version, initially zero.
    pub(crate) expected_version: u64,
    /// Durable key scoped to this method.
    pub(crate) idempotency_key: String,
}

fn required_optional<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Option::deserialize(deserializer)
}

/// Stable configuration receipt; later edits never rewrite it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Receipt {
    /// Durable operation UUID.
    pub(crate) operation_id: String,
    /// Created or edited preset UUID.
    pub(crate) agent_id: String,
    /// Exact immutable revision produced by the operation.
    pub(crate) revision: u64,
}

/// Immutable non-secret configuration revision.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(clippy::struct_field_names, reason = "field names are the wire schema")]
pub(crate) struct Agent {
    /// Stable preset identity.
    pub(crate) agent_id: String,
    /// Exact revision number.
    pub(crate) revision: u64,
    /// Frozen fields, containing references only.
    pub(crate) config: Config,
    /// Revision creation time in Unix epoch milliseconds.
    pub(crate) recorded_at: u64,
}

/// Bounded current-configuration page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Page {
    /// Current heads in stable UUID order.
    pub(crate) agents: Vec<Agent>,
    /// Exclusive cursor; a full last page may be followed by an empty page.
    pub(crate) next_after: Option<String>,
}

/// Current explicit catalog selection, or empty at initial version zero.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DefaultSelection {
    /// Selected Agent UUID or null.
    pub(crate) agent_id: Option<String>,
    /// Compare-and-swap version for the next selection change.
    pub(crate) version: u64,
}

/// Stable selection receipt; it is not a read of the current default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DefaultReceipt {
    /// Durable operation UUID.
    pub(crate) operation_id: String,
    /// Selection committed by this operation.
    pub(crate) selection: DefaultSelection,
}
