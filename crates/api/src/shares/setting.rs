//! One locally configured share and the owner's request to create or change it.

use model::ErrorCode;
use model::share::{Accepted, Audience, MAX_RECIPIENTS, Published, Visitor};
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
}

/// One share setting with its center status; serialized identically for storage and clients.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Setting {
    pub(super) workspace_key: String,
    pub(super) organization_id: Uuid,
    pub(super) permission: String,
    pub(super) audience: Audience,
    #[serde(default)]
    pub(super) recipients: Vec<Uuid>,
    #[serde(default)]
    pub(super) display_name: Option<String>,
    #[serde(default)]
    pub(super) project_name: Option<String>,
    #[serde(default)]
    pub(super) share_id: Option<Uuid>,
    #[serde(default)]
    pub(super) generation: Option<i32>,
    #[serde(default)]
    pub(super) state: ShareState,
    #[serde(default)]
    pub(super) code: Option<String>,
    #[serde(default)]
    pub(super) missing_recipients: Vec<Uuid>,
}

impl Setting {
    /// Whether this setting is the entry keyed by `workspace_key` and `organization_id`.
    pub(super) fn is(&self, workspace_key: &str, organization_id: Uuid) -> bool {
        self.workspace_key == workspace_key && self.organization_id == organization_id
    }

    /// Project this setting into the form published to the center.
    pub(super) fn published(&self) -> Published {
        Published {
            workspace_key: self.workspace_key.clone(),
            organization_id: self.organization_id,
            permission: self.permission.clone(),
            audience: self.audience,
            recipients: self.recipients.clone(),
            display_name: self.display_name.clone(),
            project_name: self.project_name.clone(),
            generation: self.generation,
        }
    }

    /// Whether an accepted share `share_id` currently admits `user_id`.
    pub(super) fn grants(&self, share_id: Uuid, user_id: Uuid) -> bool {
        self.state == ShareState::Accepted
            && self.share_id == Some(share_id)
            && (self.audience == Audience::Organization || self.recipients.contains(&user_id))
    }

    /// Whether this setting admits the guest session described by `visitor`.
    pub(super) fn admits(&self, visitor: &Visitor) -> bool {
        self.is(&visitor.workspace_key, visitor.organization_id)
            && self.permission == visitor.permission
            && self.grants(visitor.share_id, visitor.user_id)
    }

    /// Record the center's acceptance of this entry.
    pub(super) fn accept(&mut self, accepted: &Accepted) {
        self.share_id = Some(accepted.share_id);
        self.generation = Some(accepted.generation);
        self.state = ShareState::Accepted;
        self.code = None;
        self.missing_recipients
            .clone_from(&accepted.missing_recipients);
    }

    /// Record the center's refusal of this entry, keeping its identity for a later acceptance.
    pub(super) fn reject(&mut self, code: &str) {
        self.state = ShareState::Rejected;
        self.code = Some(code.to_owned());
    }
}

/// Owner request to create or change one share; the center assigns identity and generation.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SetRequest {
    pub(super) workspace_key: String,
    pub(super) organization_id: Uuid,
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
    pub(super) workspace_key: String,
    pub(super) organization_id: Uuid,
}

/// Why a share request cannot be stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(super) enum ShareError {
    /// Only [`OPERATE`] can be granted.
    #[error("only the operate permission can be shared")]
    Permission,
    /// Members audiences need recipients; organization audiences must not list any.
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
    /// The workspace does not exist or is archived.
    #[error("workspace is not active")]
    Workspace,
}

impl From<ShareError> for ErrorCode {
    fn from(error: ShareError) -> Self {
        match error {
            ShareError::Permission
            | ShareError::Audience
            | ShareError::Recipients
            | ShareError::Name => Self::InvalidMessage,
            ShareError::Capacity => Self::ResourceExhausted,
            ShareError::Workspace => Self::WorkspaceNotFound,
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
        if self.workspace_key.trim().is_empty() {
            return Err(ShareError::Workspace);
        }
        Ok(self)
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

    /// Create a new pending setting without center identity or generation.
    pub(super) fn into_setting(self) -> Setting {
        Setting {
            workspace_key: self.workspace_key,
            organization_id: self.organization_id,
            permission: self.permission,
            audience: self.audience,
            recipients: self.recipients,
            display_name: self.display_name,
            project_name: self.project_name,
            share_id: None,
            generation: None,
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
