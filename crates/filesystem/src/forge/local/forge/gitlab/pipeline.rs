//! Neutral checks and GitLab pipeline drill-down, including optional/manual jobs.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Value, json};

use super::{
    ForgeContext, ForgeRuntimeError, LocalForge, array, integer, malformed, optional_string, read,
    string, strings,
};
use crate::forge::ports::forge::{CheckDetails, CheckDetailsQuery, PullRequestCheck};

/// Fetch the MR pipeline in its source project, including fork/detached pipelines.
/// For `cwd`/`context`, prefers `mr` to the fallback `pipeline` ID and returns JSON.
/// # Errors
/// Returns missing-identity, CLI or malformed pipeline/job response errors.
pub(super) fn read_pipeline(
    forge: &LocalForge,
    cwd: &Path,
    context: &ForgeContext,
    mr: Option<u64>,
    pipeline: Option<u64>,
) -> Result<Value, ForgeRuntimeError> {
    let (selector, id) = mr
        .map(|id| ("--merge-request", id))
        .or_else(|| pipeline.map(|id| ("--pipeline-id", id)))
        .ok_or_else(|| malformed("GitLab pipeline identity is required"))?;
    let value = read(
        forge,
        cwd,
        context,
        &strings(&[
            "ci",
            "get",
            selector,
            &id.to_string(),
            "--with-job-details",
            "-F",
            "json",
        ]),
    )?;
    integer(&value, "id")?;
    string(&value, "status")?;
    for job in jobs(&value)? {
        integer(job, "id")?;
        string(job, "name")?;
        string(job, "stage")?;
        string(job, "status")?;
    }
    Ok(value)
}

/// Return the pipeline stages used by the existing GitLab detail panel.
/// Resolves `query` in `cwd`/`context` through `forge` and projects neutral details.
/// # Errors
/// Returns missing-identity, CLI or malformed pipeline/job response errors.
pub(in super::super) fn check_details(
    forge: &LocalForge,
    cwd: &Path,
    context: &ForgeContext,
    query: CheckDetailsQuery<'_>,
) -> Result<CheckDetails, ForgeRuntimeError> {
    let value = read_pipeline(
        forge,
        cwd,
        context,
        query.change_request_number,
        query.workflow_run_id.or(query.check_run_id),
    )?;
    let id = integer(&value, "id")?;
    let reference = optional_string(&value, "ref");
    let url = optional_string(&value, "web_url");
    Ok(CheckDetails {
        check_run_id: id,
        workflow_run_id: None,
        name: reference.as_ref().map_or_else(
            || format!("Pipeline #{id}"),
            |reference| format!("Pipeline ({reference})"),
        ),
        status: None,
        conclusion: None,
        url: url.clone(),
        details_url: url,
        output: None,
        annotations: Vec::new(),
        failed_jobs: Vec::new(),
        truncated: false,
        pipeline: Some(details(&value)?),
    })
}

fn jobs(value: &Value) -> Result<&[Value], ForgeRuntimeError> {
    array(
        value
            .get("jobs")
            .ok_or_else(|| malformed("GitLab pipeline jobs are missing"))?,
    )
}

/// GitLab statuses for which auto-merge schedules rather than immediately merging.
pub(super) fn is_active(status: &str) -> bool {
    matches!(
        status,
        "created"
            | "waiting_for_resource"
            | "preparing"
            | "pending"
            | "running"
            | "canceling"
            | "scheduled"
    )
}

/// The pipeline's own aggregate is authoritative, even with manual deployment jobs.
pub(super) fn checks_status(status: &str) -> &'static str {
    match status {
        "success" | "passed" => "success",
        "failed" => "failure",
        "manual" => "pending",
        _ if is_active(status) => "pending",
        _ => "none",
    }
}

fn job_status(status: &str) -> &'static str {
    match status {
        "success" | "passed" => "success",
        "failed" => "failed",
        "running" => "running",
        "pending" => "pending",
        "created" => "created",
        "canceled" | "cancelled" => "canceled",
        "skipped" => "skipped",
        "manual" => "manual",
        _ if is_active(status) => "pending",
        _ => "unknown",
    }
}

/// Preserve manual and allowed-failure distinctions in neutral check traits.
/// Returns `pipeline` jobs ordered by numeric ID.
/// # Errors
/// Rejects missing or incorrectly typed pipeline/job fields.
pub(super) fn checks(pipeline: &Value) -> Result<Vec<PullRequestCheck>, ForgeRuntimeError> {
    let mut result = Vec::new();
    for job in jobs(pipeline)? {
        let raw = string(job, "status")?;
        let optional = job
            .get("allow_failure")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let (status, traits) = match (job_status(&raw), optional) {
            ("failed", true) => ("success", Some(vec!["warning".to_owned()])),
            ("manual", true) => ("skipped", Some(vec!["manual".to_owned()])),
            ("manual", false) => (
                "pending",
                Some(vec!["manual".to_owned(), "action_required".to_owned()]),
            ),
            ("failed", false) => ("failure", None),
            ("canceled", _) => ("cancelled", None),
            ("success", _) => ("success", None),
            ("skipped", _) => ("skipped", None),
            _ => ("pending", None),
        };
        result.push(PullRequestCheck {
            name: string(job, "name")?,
            status: status.to_owned(),
            url: optional_string(job, "web_url"),
            workflow: optional_string(job, "stage"),
            duration: None,
            check_run_id: Some(integer(job, "id")?),
            workflow_run_id: Some(integer(pipeline, "id")?),
            traits,
        });
    }
    result.sort_by_key(|check| check.check_run_id);
    Ok(result)
}

fn details(pipeline: &Value) -> Result<Value, ForgeRuntimeError> {
    let mut ordered: Vec<_> = jobs(pipeline)?.iter().collect();
    ordered.sort_by_key(|job| job.get("id").and_then(Value::as_u64));
    let mut stages: Vec<Value> = Vec::new();
    let mut stage_indices: BTreeMap<String, usize> = BTreeMap::new();
    for job in ordered {
        let stage = string(job, "stage")?;
        let raw = string(job, "status")?;
        let item = json!({
            "id": integer(job, "id")?,
            "name": string(job, "name")?,
            "stage": stage,
            "status": job_status(&raw),
            "rawStatus": raw,
            "url": job.get("web_url"),
            "allowFailure": job.get("allow_failure").and_then(Value::as_bool).unwrap_or(false),
            "durationSeconds": job.get("duration")
        });
        if let Some(index) = stage_indices.get(&stage).copied() {
            stages[index]["jobs"]
                .as_array_mut()
                .expect("stage jobs are always an array")
                .push(item);
        } else {
            stage_indices.insert(stage.clone(), stages.len());
            stages.push(json!({"name":stage,"status":"unknown","jobs":[item]}));
        }
    }
    for stage in &mut stages {
        let values = stage["jobs"]
            .as_array()
            .expect("stage jobs are always an array");
        let status = [
            "running", "failed", "pending", "created", "manual", "canceled", "skipped", "success",
        ]
        .into_iter()
        .find(|candidate| {
            values.iter().any(|job| {
                let status = job["status"].as_str().unwrap_or("unknown");
                let effective =
                    if job["allowFailure"] == true && matches!(status, "failed" | "manual") {
                        "success"
                    } else {
                        status
                    };
                effective == *candidate
            })
        })
        .unwrap_or("unknown");
        stage["status"] = json!(status);
    }
    let raw = string(pipeline, "status")?;
    Ok(json!({
        "id": integer(pipeline, "id")?,
        "status": job_status(&raw),
        "rawStatus": raw,
        "url": pipeline.get("web_url"),
        "ref": pipeline.get("ref"),
        "sha": pipeline.get("sha"),
        "stages": stages
    }))
}
