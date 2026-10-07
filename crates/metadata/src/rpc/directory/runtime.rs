//! Checkout facts projected into the existing Workspace wire shape.

use model::workspace::protocol::workspace::{
    AheadBehind, CheckStatus, ChecksStatus, DiffStat, Mergeable, ReviewDecision, WorkspaceCheck,
    WorkspaceDescriptorPayload, WorkspaceGitHubRuntimePayload, WorkspaceGitRuntimePayload,
    WorkspacePullRequest, WorkspaceRuntimeError,
};
use model::workspace::runtime::{WorkspacePullRequestSnapshot, WorkspaceRuntimeSnapshot};

/// Overlay cached facts onto `descriptor`, explicitly clearing fields absent from `snapshot`.
pub(super) fn apply(
    descriptor: &mut WorkspaceDescriptorPayload,
    snapshot: &WorkspaceRuntimeSnapshot,
) {
    descriptor.diff_stat = snapshot.git.as_ref().and_then(|git| {
        git.diff_stat.map(|stat| DiffStat {
            additions: stat.additions.into(),
            deletions: stat.deletions.into(),
        })
    });
    descriptor.git_runtime = snapshot.git.as_ref().map(|git| WorkspaceGitRuntimePayload {
        current_branch: git.current_branch.clone(),
        remote_url: git.remote_url.clone(),
        is_paseo_owned_worktree: Some(git.is_managed_worktree),
        is_dirty: git.is_dirty,
        ahead_behind: git.ahead_behind.map(|(ahead, behind)| AheadBehind {
            ahead: ahead.into(),
            behind: behind.into(),
        }),
        ahead_of_origin: git.ahead_of_origin.map(Into::into),
        behind_of_origin: git.behind_of_origin.map(Into::into),
    });
    descriptor.github_runtime =
        snapshot
            .forge
            .as_ref()
            .map(|forge| WorkspaceGitHubRuntimePayload {
                features_enabled: Some(forge.features_enabled),
                pull_request: forge.pull_request.as_ref().map(pull_request),
                error: forge.error.as_ref().map(|message| WorkspaceRuntimeError {
                    message: message.clone(),
                }),
                refreshed_at: None,
            });
    descriptor.forge = snapshot
        .forge
        .as_ref()
        .and_then(|forge| forge.forge.clone());
}

fn pull_request(pr: &WorkspacePullRequestSnapshot) -> WorkspacePullRequest {
    WorkspacePullRequest {
        number: pr.number.map(Into::into),
        url: pr.url.clone(),
        title: pr.title.clone(),
        state: pr.state.clone(),
        base_ref_name: pr.base_ref_name.clone(),
        head_ref_name: pr.head_ref_name.clone(),
        is_merged: pr.is_merged,
        is_draft: Some(pr.is_draft),
        mergeable: Some(match pr.mergeable.as_str() {
            "MERGEABLE" => Mergeable::Mergeable,
            "CONFLICTING" => Mergeable::Conflicting,
            _ => Mergeable::Unknown,
        }),
        checks: Some(
            pr.checks
                .iter()
                .map(|check| WorkspaceCheck {
                    name: check.name.clone(),
                    status: match check.status.as_str() {
                        "success" => CheckStatus::Success,
                        "failure" => CheckStatus::Failure,
                        "skipped" => CheckStatus::Skipped,
                        "cancelled" => CheckStatus::Cancelled,
                        _ => CheckStatus::Pending,
                    },
                    url: check.url.clone(),
                    workflow: check.workflow.clone(),
                    duration: check.duration.clone(),
                    traits: check.traits.clone(),
                })
                .collect(),
        ),
        checks_status: Some(match pr.checks_status.as_str() {
            "success" => ChecksStatus::Success,
            "failure" => ChecksStatus::Failure,
            "pending" => ChecksStatus::Pending,
            _ => ChecksStatus::None,
        }),
        review_decision: pr
            .review_decision
            .as_deref()
            .map(|decision| match decision {
                "approved" => ReviewDecision::Approved,
                "changes_requested" => ReviewDecision::ChangesRequested,
                _ => ReviewDecision::Pending,
            }),
        repo_owner: pr.repo_owner.clone(),
        repo_name: pr.repo_name.clone(),
        github: pr.github.clone(),
    }
}
