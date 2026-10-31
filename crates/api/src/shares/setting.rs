//! One locally configured share and the owner's request to create or change it.

use model::ErrorCode;
use model::share::{
    Accepted, Audience, MAX_RECIPIENTS, Published, ResourceKind, ShareScope, Visitor,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The only permission a share can grant today.
pub(super) const OPERATE: &str = "operate";

/// Longest accepted display or project name, in characters.
const MAX_NAME_CHARS: usize = 200;

/// Center status of one local setting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ShareState {
    /// Not yet confirmed by the center.
    #[default]
    Pending,
    /// The center accepted the latest publication of this entry.
    Accepted,
    /// The center refused the entry; see its code.
    Rejected,
    /// The owner revoked the share from another device. Kept so the owner sees why it
    /// stopped; never published and never admits anyone. Sharing again creates a new setting.
    Revoked,
}

/// One share setting with its center status; serialized identically for storage and clients.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Setting {
    pub(super) resource_kind: ResourceKind,
    #[serde(default)]
    pub(super) resource_key: String,
    pub(super) group_id: Uuid,
    pub(super) permission: String,
    pub(super) audience: Audience,
    #[serde(default)]
    pub(super) recipients: Vec<Uuid>,
    #[serde(default)]
    pub(super) display_name: Option<String>,
    #[serde(default)]
    pub(super) project_name: Option<String>,
    /// Generated when the user creates the setting; the center rejects it for good once
    /// the owner revokes it. Required: a stored setting without one could be revoked.
    #[serde(rename = "settingId")]
    pub(super) id: Uuid,
    #[serde(default)]
    pub(super) share_id: Option<Uuid>,
    #[serde(default)]
    pub(super) state: ShareState,
    #[serde(default)]
    pub(super) code: Option<String>,
    #[serde(default)]
    pub(super) missing_recipients: Vec<Uuid>,
}

impl Setting {
    /// Whether this setting is the entry keyed by `kind`, `key` and `group_id`.
    pub(super) fn is(&self, kind: ResourceKind, key: &str, group_id: Uuid) -> bool {
        self.resource_kind == kind && self.resource_key == key && self.group_id == group_id
    }

    /// Project this setting into the form published to the center.
    pub(super) fn published(&self) -> Published {
        Published {
            resource_kind: self.resource_kind,
            resource_key: self.resource_key.clone(),
            group_id: self.group_id,
            permission: self.permission.clone(),
            audience: self.audience,
            recipients: self.recipients.clone(),
            display_name: self.display_name.clone(),
            project_name: self.project_name.clone(),
            setting_id: self.id,
        }
    }

    /// Whether an accepted share `share_id` currently admits `user_id`. Recipients the
    /// center reported as not being members are not admitted until it accepts them again.
    pub(super) fn grants(&self, share_id: Uuid, user_id: Uuid) -> bool {
        self.state == ShareState::Accepted
            && self.share_id == Some(share_id)
            && (self.audience == Audience::Group || self.recipients.contains(&user_id))
            && !self.missing_recipients.contains(&user_id)
    }

    /// Whether this setting admits the guest session described by `visitor`.
    pub(super) fn admits(&self, visitor: &Visitor) -> bool {
        self.is(
            visitor.resource_kind,
            &visitor.resource_key,
            visitor.group_id,
        ) && self.permission == visitor.permission
            && self.grants(visitor.share_id, visitor.user_id)
    }

    /// Record the center's acceptance of this entry.
    pub(super) fn accept(&mut self, accepted: &Accepted) {
        self.share_id = Some(accepted.share_id);
        self.state = ShareState::Accepted;
        self.code = None;
        self.missing_recipients
            .clone_from(&accepted.missing_recipients);
    }

    /// Whether the center still hears about this setting; revoked ones stay local.
    pub(super) fn publishable(&self) -> bool {
        self.state != ShareState::Revoked
    }

    /// Record the owner's revocation from another device.
    pub(super) fn revoke(&mut self) {
        self.state = ShareState::Revoked;
        self.code = Some("revoked".to_owned());
        self.missing_recipients.clear();
    }

    /// Record the center's refusal of this entry, keeping its identity for a later acceptance.
    pub(super) fn reject(&mut self, code: &str) {
        self.state = ShareState::Rejected;
        self.code = Some(code.to_owned());
    }
}

/// Owner request to create or change one share; the center assigns its identity.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SetRequest {
    pub(super) resource_kind: ResourceKind,
    #[serde(default)]
    pub(super) resource_key: String,
    pub(super) group_id: Uuid,
    pub(super) permission: String,
    pub(super) audience: Audience,
    #[serde(default)]
    pub(super) recipients: Vec<Uuid>,
    #[serde(default)]
    pub(super) display_name: Option<String>,
    #[serde(default)]
    pub(super) project_name: Option<String>,
}

/// Owner request naming one share to remove.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RemoveRequest {
    pub(super) resource_kind: ResourceKind,
    #[serde(default)]
    pub(super) resource_key: String,
    pub(super) group_id: Uuid,
}

/// Why a share request cannot be stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(super) enum ShareError {
    /// Only [`OPERATE`] can be granted.
    #[error("only the operate permission can be shared")]
    Permission,
    /// Members audiences need recipients; group audiences must not list any.
    #[error("recipients do not match the audience")]
    Audience,
    /// More than [`MAX_RECIPIENTS`] recipients.
    #[error("too many recipients")]
    Recipients,
    /// A name is blank, too long, or contains control characters.
    #[error("invalid display or project name")]
    Name,
    /// The Host already has [`model::share::MAX_SHARES`] shares.
    #[error("too many shares")]
    Capacity,
    /// The resource key does not fit its kind: blank for a workspace or project, or not
    /// empty for a node.
    #[error("resource key does not match its kind")]
    Resource,
    /// The workspace does not exist or is archived.
    #[error("workspace is not active")]
    Workspace,
    /// The project does not exist or is archived.
    #[error("project is not active")]
    Project,
}

impl From<ShareError> for ErrorCode {
    fn from(error: ShareError) -> Self {
        match error {
            ShareError::Permission
            | ShareError::Audience
            | ShareError::Recipients
            | ShareError::Name
            | ShareError::Resource => Self::InvalidMessage,
            ShareError::Capacity => Self::ResourceExhausted,
            ShareError::Workspace | ShareError::Project => Self::WorkspaceNotFound,
        }
    }
}

impl SetRequest {
    /// Validate the request and normalize recipients to a sorted, duplicate-free list.
    ///
    /// # Errors
    /// Returns the first [`ShareError`] that makes the request unstorable.
    pub(super) fn normalize(mut self) -> Result<Self, ShareError> {
        if self.permission != OPERATE {
            return Err(ShareError::Permission);
        }
        self.recipients.sort_unstable();
        self.recipients.dedup();
        if self.recipients.len() > MAX_RECIPIENTS {
            return Err(ShareError::Recipients);
        }
        let listed = !self.recipients.is_empty();
        if listed != (self.audience == Audience::Members) {
            return Err(ShareError::Audience);
        }
        self.display_name = name(self.display_name)?;
        self.project_name = name(self.project_name)?;
        if self.scope().is_none() {
            return Err(ShareError::Resource);
        }
        Ok(self)
    }

    /// The shared resource; `None` when kind and key do not fit together.
    pub(super) fn scope(&self) -> Option<ShareScope> {
        ShareScope::new(self.resource_kind, &self.resource_key)
    }

    /// Write this request over `setting`, returning whether anything published changed.
    pub(super) fn update(self, setting: &mut Setting) -> bool {
        let changed = setting.permission != self.permission
            || setting.audience != self.audience
            || setting.recipients != self.recipients
            || setting.display_name != self.display_name
            || setting.project_name != self.project_name;
        if changed {
            setting.permission = self.permission;
            setting.audience = self.audience;
            setting.recipients = self.recipients;
            setting.display_name = self.display_name;
            setting.project_name = self.project_name;
            if setting.state == ShareState::Rejected {
                setting.state = ShareState::Pending;
                setting.code = None;
            }
        }
        changed
    }

    /// Create a new pending setting with a fresh setting ID and no center identity.
    pub(super) fn into_setting(self) -> Setting {
        Setting {
            resource_kind: self.resource_kind,
            resource_key: self.resource_key,
            group_id: self.group_id,
            permission: self.permission,
            audience: self.audience,
            recipients: self.recipients,
            display_name: self.display_name,
            project_name: self.project_name,
            id: Uuid::new_v4(),
            share_id: None,
            state: ShareState::Pending,
            code: None,
            missing_recipients: Vec::new(),
        }
    }
}

fn name(value: Option<String>) -> Result<Option<String>, ShareError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > MAX_NAME_CHARS || trimmed.chars().any(char::is_control) {
        return Err(ShareError::Name);
    }
    Ok(Some(trimmed.to_owned()))
}

#[cfg(test)]
mod tests;
