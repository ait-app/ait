//! Checkout facts projected into the existing Workspace wire shape.

use domain::workspace::protocol::workspace::{
    AheadBehind, CheckStatus, ChecksStatus, DiffStat, Mergeable, ReviewDecision, WorkspaceCheck,
    WorkspaceDescriptorPayload, WorkspaceGitHubRuntimePayload, WorkspaceGitRuntimePayload,
    WorkspacePullRequest, WorkspaceRuntimeError,
};
use domain::workspace::runtime::{
    WorkspaceCheckSnapshot, WorkspacePullRequestSnapshot, WorkspaceRuntimeSnapshot,
};

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
        checks: Some(pr.checks.iter().map(summary_check).collect()),
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

/// Keep only what sidebar and hover-card summaries count: name, status and traits.
///
/// Every listed Workspace carries its checks, and large CI matrices (100+ checks per PR)
/// otherwise push one `workspace.list` response past the relay's 1 MiB record limit.
/// Links, workflows and durations stay available through `checkout.pr.status.request`.
fn summary_check(check: &WorkspaceCheckSnapshot) -> WorkspaceCheck {
    WorkspaceCheck {
        name: check.name.clone(),
        status: match check.status.as_str() {
            "success" => CheckStatus::Success,
            "failure" => CheckStatus::Failure,
            "skipped" => CheckStatus::Skipped,
            "cancelled" => CheckStatus::Cancelled,
            _ => CheckStatus::Pending,
        },
        url: None,
        workflow: None,
        duration: None,
        traits: check.traits.clone(),
    }
}
