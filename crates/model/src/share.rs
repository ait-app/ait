//! Resource sharing ports shared by the hub serve link, the local API and the daemon.
//!
//! A Host owns its share settings. Each share grants one group access to a workspace, a
//! project, or the whole node (Host). The daemon publishes the settings to the center over
//! the hub serve link (`hub.shares.publish`) and applies the center's outcome. Guest
//! sessions arrive on the same link with a [`Visitor`]; the bridge hands the visitor to
//! the local API in [`GUEST_HEADER`], and the API confines that connection to the shared
//! [`ShareScope`].

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

/// Kind of resource a share grants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    /// One workspace; the key is its workspace ID.
    Workspace,
    /// One project with all its current and future workspaces; the key is its project ID.
    Project,
    /// The whole node (Host); the key is empty.
    Node,
}

/// The resource one share grants, validated from a kind and key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShareScope {
    /// One workspace, by workspace ID.
    Workspace(String),
    /// One project, by project ID.
    Project(String),
    /// Every project and workspace on this Host.
    Node,
}

impl ShareScope {
    /// Build a scope from its wire form.
    ///
    /// # Arguments
    ///
    /// * `kind` - Resource kind of the share
    /// * `key` - Workspace or project ID; empty for a node share
    ///
    /// # Returns
    ///
    /// `None` when a workspace or project key is blank or a node key is not empty.
    #[must_use]
    pub fn new(kind: ResourceKind, key: &str) -> Option<Self> {
        match kind {
            ResourceKind::Node => key.is_empty().then_some(Self::Node),
            ResourceKind::Workspace if !key.trim().is_empty() => {
                Some(Self::Workspace(key.to_owned()))
            }
            ResourceKind::Project if !key.trim().is_empty() => Some(Self::Project(key.to_owned())),
            ResourceKind::Workspace | ResourceKind::Project => None,
        }
    }

    /// Resource kind of this scope.
    #[must_use]
    pub const fn kind(&self) -> ResourceKind {
        match self {
            Self::Workspace(_) => ResourceKind::Workspace,
            Self::Project(_) => ResourceKind::Project,
            Self::Node => ResourceKind::Node,
        }
    }

    /// Resource key of this scope; empty for [`ShareScope::Node`].
    #[must_use]
    pub fn key(&self) -> &str {
        match self {
            Self::Workspace(key) | Self::Project(key) => key,
            Self::Node => "",
        }
    }
}

/// Who opened a guest session and what they may reach, as asserted by the center.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Visitor {
    /// Visiting account.
    pub user_id: Uuid,
    /// Display name for attribution; never used for authorization.
    pub display_name: Option<String>,
    /// Group through which the share is granted.
    pub group_id: Uuid,
    /// Center share identity.
    pub share_id: Uuid,
    /// Kind of the shared resource.
    pub resource_kind: ResourceKind,
    /// Workspace or project ID; empty for a node share.
    pub resource_key: String,
    /// Granted permission; only `operate` exists today.
    pub permission: String,
}

impl Visitor {
    /// Encode for [`GUEST_HEADER`] as unpadded URL-safe base64 of the JSON form.
    ///
    /// # Panics
    /// Never in practice: every field is a string, UUID, enum or option of one.
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

    /// The shared resource, or `None` when kind and key do not form a valid scope.
    #[must_use]
    pub fn scope(&self) -> Option<ShareScope> {
        ShareScope::new(self.resource_kind, &self.resource_key)
    }
}

/// Who may see a share within its group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Audience {
    /// Every active member of the group.
    Group,
    /// Only the listed recipients.
    Members,
}

/// One share setting as the daemon publishes it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Published {
    /// Kind of the shared resource.
    pub resource_kind: ResourceKind,
    /// Workspace or project ID, unique on this Host; empty for a node share.
    pub resource_key: String,
    /// Group the resource is shared into.
    pub group_id: Uuid,
    /// Granted permission; only `operate` exists today.
    pub permission: String,
    /// Whole group or listed members.
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
    /// Resource kind of the entry.
    pub resource_kind: ResourceKind,
    /// Resource key of the entry.
    pub resource_key: String,
    /// Group of the entry.
    pub group_id: Uuid,
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
    /// Resource kind of the entry.
    pub resource_kind: ResourceKind,
    /// Resource key of the entry.
    pub resource_key: String,
    /// Group of the entry.
    pub group_id: Uuid,
    /// Reason: `group_inactive`, `not_member`, `revoked`, `missing_name` or `no_recipients`.
    pub code: String,
}

/// A group the Host owner belongs to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    /// Group identity.
    pub group_id: Uuid,
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
    /// The owner's current groups.
    pub groups: Vec<Group>,
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
    /// removal of revoked ones, and the owner's groups. Guest sessions of shares that
    /// are no longer accepted end, with their terminals and agent runs.
    fn apply(&self, outcome: Outcome);
}

#[cfg(test)]
mod tests;
