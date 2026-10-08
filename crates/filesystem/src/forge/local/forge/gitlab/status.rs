//! MR status and forge facts consumed by Paseo's GitLab UI.

use serde_json::{Value, json};

use super::{
    ForgeContext, ForgeRuntimeError, integer, optional_string, pipeline, project_path, string,
};
use crate::forge::ports::forge::{PullRequestMergeable, PullRequestStatus};

/// Project GitLab's status without inferring merge readiness from CI alone.
/// Uses MR JSON and resolved project context, with optional approval/job enrichment.
/// # Errors
/// Rejects missing or incorrectly typed required MR and job fields.
pub(super) fn parse(
    mr: &Value,
    context: &ForgeContext,
    approvals: Option<&Value>,
    pipeline: Option<&Value>,
) -> Result<PullRequestStatus, ForgeRuntimeError> {
    let state = string(mr, "state")?;
    let path = project_path(mr, context);
    let (owner, name) = path.rsplit_once('/').unwrap_or(("", path));
    let required = approvals
        .and_then(|value| value.get("approvals_required"))
        .or_else(|| mr.get("approvals_required"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let given = approvals
        .and_then(|value| value.get("approved_by"))
        .and_then(Value::as_array)
        .map(Vec::len)
        .map(|count| count as u64)
        .or_else(|| {
            approvals
                .and_then(|value| value.get("approvals_left"))
                .and_then(Value::as_u64)
                .map(|left| required.saturating_sub(left))
        })
        .or_else(|| mr.get("approvals_given").and_then(Value::as_u64))
        .unwrap_or(0);
    let conflicts = mr
        .get("has_conflicts")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let detailed = mr.get("detailed_merge_status").and_then(Value::as_str);
    let ready = detailed.map_or_else(
        || mr.get("merge_status").and_then(Value::as_str) == Some("can_be_merged"),
        |status| status == "mergeable",
    );
    let checks_status = pipeline
        .and_then(|value| value.get("status"))
        .or_else(|| mr.pointer("/head_pipeline/status"))
        .and_then(Value::as_str)
        .map_or("none", pipeline::checks_status);
    Ok(PullRequestStatus {
        forge: "gitlab".to_owned(),
        project_path: Some(path.to_owned()),
        number: Some(integer(mr, "iid")?),
        url: string(mr, "web_url")?,
        title: string(mr, "title")?,
        state: if state == "opened" {
            "open".to_owned()
        } else {
            state.clone()
        },
        base_ref_name: string(mr, "target_branch")?,
        head_ref_name: string(mr, "source_branch")?,
        head_sha: optional_string(mr, "sha"),
        is_merged: state == "merged" || optional_string(mr, "merged_at").is_some(),
        is_draft: mr
            .get("draft")
            .and_then(Value::as_bool)
            .or_else(|| mr.get("work_in_progress").and_then(Value::as_bool))
            .unwrap_or(false),
        mergeable: if conflicts {
            PullRequestMergeable::Conflicting
        } else if ready {
            PullRequestMergeable::Mergeable
        } else {
            PullRequestMergeable::Unknown
        },
        checks: pipeline
            .map(pipeline::checks)
            .transpose()?
            .unwrap_or_default(),
        checks_status: checks_status.to_owned(),
        review_decision: None,
        // Legacy repoOwner is a single segment; projectPath carries nested groups.
        repo_owner: owner
            .split('/')
            .next()
            .filter(|owner| !owner.is_empty())
            .map(str::to_owned),
        repo_name: Some(name.to_owned()),
        github: None,
        forge_specific: Some(json!({
            "forge": "gitlab",
            "detailedMergeStatus": detailed,
            "mergeStatus": mr.get("merge_status"),
            "hasConflicts": conflicts,
            "blockingDiscussionsResolved": mr.get("blocking_discussions_resolved")
                .and_then(Value::as_bool).unwrap_or(true),
            "approvalsRequired": required,
            "approvalsGiven": given,
            "pipelineStatus": mr.pointer("/head_pipeline/status"),
            "pipelineId": mr.pointer("/head_pipeline/id"),
            "pipelineUrl": mr.pointer("/head_pipeline/web_url"),
            "mergeWhenPipelineSucceeds": mr.get("merge_when_pipeline_succeeds")
                .and_then(Value::as_bool).unwrap_or(false)
        })),
    })
}
