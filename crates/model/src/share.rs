//! Workspace sharing ports shared by the hub serve link, the local API and the daemon.
//!
//! A Host owns its share settings. The daemon publishes them to the center over the
//! hub serve link (`hub.shares.publish`) and applies the center's outcome. Guest
//! sessions arrive on the same link with a [`Visitor`]; the bridge hands the visitor
//! to the local API in [`GUEST_HEADER`], and the API confines that connection to one
//! workspace.

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use uuid::Uuid;

/// Loopback upgrade header carrying a guest session's [`Visitor`]. The local API
/// honours it only together with the daemon's own bearer credential.
pub const GUEST_HEADER: &str = "x-ait-guest";

/// Center method that replaces this Host's published share list.
pub const PUBLISH_METHOD: &str = "hub.shares.publish";

/// A share admits at most this many entries per Host, matching the center.
pub const MAX_SHARES: usize = 64;

/// A share names at most this many recipients, matching the center.
pub const MAX_RECIPIENTS: usize = 100;

/// Who opened a guest session and what they may reach, as asserted by the center.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Visitor {
    /// Visiting account.
    pub user_id: Uuid,
    /// Display name for attribution; never used for authorization.
    pub display_name: Option<String>,
    /// Organization through which the share is granted.
    pub organization_id: Uuid,
    /// Center share identity.
    pub share_id: Uuid,
    /// The only workspace this session may use.
    pub workspace_key: String,
    /// Granted permission; only `operate` exists today.
    pub permission: String,
}

impl Visitor {
    /// Encode for [`GUEST_HEADER`] as unpadded URL-safe base64 of the JSON form.
    ///
    /// # Panics
    /// Never in practice: every field is a string, UUID or option of one.
    #[must_use]
    pub fn encode(&self) -> String {
        let json = serde_json::to_vec(self).expect("visitor fields serialize to JSON");
        URL_SAFE_NO_PAD.encode(json)
    }

    /// Decode a [`GUEST_HEADER`] value; malformed values yield `None`.
    #[must_use]
    pub fn decode(value: &str) -> Option<Self> {
        let json = URL_SAFE_NO_PAD.decode(value).ok()?;
        serde_json::from_slice(&json).ok()
    }
}

/// Who may see a share within its organization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Audience {
    /// Every active member of the organization.
    Organization,
    /// Only the listed recipients.
    Members,
}

/// One share setting as the daemon publishes it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Published {
    /// AIT workspace ID, unique on this Host.
    pub workspace_key: String,
    /// Organization the workspace is shared into.
    pub organization_id: Uuid,
    /// Granted permission; only `operate` exists today.
    pub permission: String,
    /// Whole organization or listed members.
    pub audience: Audience,
    /// Recipients for [`Audience::Members`]; empty otherwise.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recipients: Vec<Uuid>,
    /// Name shown to visitors; the center falls back to the directory title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Project name shown to visitors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
    /// Generation from the last acceptance; absent for a share the user just created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation: Option<i32>,
}

/// A published entry the center accepted.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Accepted {
    /// Stable center identity.
    pub share_id: Uuid,
    /// Workspace of the entry.
    pub workspace_key: String,
    /// Organization of the entry.
    pub organization_id: Uuid,
    /// Generation to send with this entry from now on.
    pub generation: i32,
    /// Recipients that are not active members right now.
    #[serde(default)]
    pub missing_recipients: Vec<Uuid>,
}

/// A published entry the center refused.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rejected {
    /// Workspace of the entry.
    pub workspace_key: String,
    /// Organization of the entry.
    pub organization_id: Uuid,
    /// Reason, e.g. `not_member`, `org_inactive`, `revoked`, `no_recipients`.
    pub code: String,
}

/// An organization the Host owner belongs to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Organization {
    /// Organization identity.
    pub organization_id: Uuid,
    /// Display name.
    pub name: String,
    /// `active` or `suspended`.
    pub status: String,
    /// The owner's role, `admin` or `member`.
    pub my_role: String,
}

/// The center's outcome for the latest publication, from a reply or a `share_status` frame.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    /// Revision the outcome applies to; absent in a call reply.
    #[serde(default)]
    pub revision: Option<u64>,
    /// Accepted entries with their identities.
    pub accepted: Vec<Accepted>,
    /// Refused entries with reasons.
    pub rejected: Vec<Rejected>,
    /// The owner's current organizations.
    pub orgs: Vec<Organization>,
}

/// Daemon-owned share settings, implemented by the local API.
///
/// The publication loop reads [`ShareBook::publication`] whenever
/// [`ShareBook::changes`] advances or a fresh serve link appears, and feeds every
/// center outcome back through [`ShareBook::apply`].
pub trait ShareBook: fmt::Debug + Send + Sync {
    /// The complete list to publish, with a revision that grows with every change.
    fn publication(&self) -> (u64, Vec<Published>);

    /// Advances whenever the list to publish changes.
    fn changes(&self) -> watch::Receiver<u64>;

    /// Record the center's outcome: identities and generations of accepted entries,
    /// removal of revoked ones, and the owner's organizations. Guest sessions of
    /// shares that are no longer accepted end, with their terminals and agent runs.
    fn apply(&self, outcome: Outcome);
}

#[cfg(test)]
mod tests;
