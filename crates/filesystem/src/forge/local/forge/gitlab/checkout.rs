//! Paseo's MR ref and source-branch fallback for managed GitLab checkouts.

use std::path::Path;

use serde_json::Value;

use super::{ForgeContext, LocalForge, integer, string, view};
use crate::worktrees::ports::worktrees::{
    ChangeRequestCheckout, ChangeRequestCheckoutRef, WorktreeError,
};

/// Resolve `number` in `cwd`/`context`, optionally overriding the local head name.
/// Returns ordered MR/source refs and preserves the fork setup trust boundary.
/// # Errors
/// Returns unavailable CLI/MR or malformed checkout metadata errors.
pub(in super::super) fn checkout(
    forge: &LocalForge,
    cwd: &Path,
    context: &ForgeContext,
    number: u64,
    head_ref: Option<&str>,
) -> Result<ChangeRequestCheckout, WorktreeError> {
    let failed = || WorktreeError::Io("Unable to resolve merge request through GitLab CLI".into());
    let mr = view(forge, cwd, context, number).map_err(|_| failed())?;
    if integer(&mr, "iid").map_err(|_| failed())? != number {
        return Err(failed());
    }
    let source = string(&mr, "source_branch").map_err(|_| failed())?;
    let head = head_ref
        .map(str::trim)
        .filter(|head| !head.is_empty())
        .unwrap_or(&source);
    let source_project = mr.get("source_project_id").and_then(Value::as_u64);
    let target_project = mr.get("target_project_id").and_then(Value::as_u64);
    let fork = source_project
        .zip(target_project)
        .filter(|(source, target)| source != target);
    Ok(ChangeRequestCheckout {
        forge: "gitlab".to_owned(),
        number,
        head_ref: head.to_owned(),
        base_ref: string(&mr, "target_branch").map_err(|_| failed())?,
        local_branch: head.to_owned(),
        untrusted_repository: fork.map(|(source, _)| format!("GitLab project {source}")),
        push_remote_url: None,
        track_origin: fork.is_none(),
        checkout_refs: [
            format!("refs/merge-requests/{number}/head"),
            format!("refs/heads/{source}"),
        ]
        .into_iter()
        .map(|reference| ChangeRequestCheckoutRef {
            remote: "origin".to_owned(),
            reference,
        })
        .collect(),
    })
}
