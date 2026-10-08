//! Existing Forge outcomes mapped into the metadata consumer's contract.

use domain::workspace::runtime::{
    WorkspaceCheckSnapshot, WorkspaceForgeSnapshot, WorkspacePullRequestSnapshot,
};

use crate::forge::ports::forge::{
    ForgeAuthState, ForgeFailureKind, ForgeRuntimeError, PullRequestMergeable, PullRequestStatus,
    PullRequestStatusRead,
};

pub(super) fn snapshot(
    read: Result<PullRequestStatusRead, ForgeRuntimeError>,
) -> WorkspaceForgeSnapshot {
    match read {
        Ok(read) => WorkspaceForgeSnapshot {
            features_enabled: matches!(
                read.auth_state,
                ForgeAuthState::Authenticated | ForgeAuthState::Error
            ),
            forge: read.forge,
            pull_request: read.status.map(pull_request),
            error: None,
        },
        Err(error) => WorkspaceForgeSnapshot {
            features_enabled: matches!(
                error.kind,
                ForgeFailureKind::Unknown | ForgeFailureKind::Forbidden
            ),
            forge: error.forge,
            pull_request: None,
            error: Some(error.message),
        },
    }
}

fn pull_request(pr: PullRequestStatus) -> WorkspacePullRequestSnapshot {
    WorkspacePullRequestSnapshot {
        number: pr.number,
        url: pr.url,
        title: pr.title,
        state: pr.state,
        base_ref_name: pr.base_ref_name,
        head_ref_name: pr.head_ref_name,
        is_merged: pr.is_merged,
        is_draft: pr.is_draft,
        mergeable: match pr.mergeable {
            PullRequestMergeable::Mergeable => "MERGEABLE",
            PullRequestMergeable::Conflicting => "CONFLICTING",
            PullRequestMergeable::Unknown => "UNKNOWN",
        }
        .to_owned(),
        checks: pr
            .checks
            .into_iter()
            .map(|check| WorkspaceCheckSnapshot {
                name: check.name,
                status: check.status,
                url: check.url,
                workflow: check.workflow,
                duration: check.duration,
                traits: check.traits,
            })
            .collect(),
        checks_status: pr.checks_status,
        review_decision: pr.review_decision,
        repo_owner: pr.repo_owner,
        repo_name: pr.repo_name,
        github: pr.github,
    }
}
