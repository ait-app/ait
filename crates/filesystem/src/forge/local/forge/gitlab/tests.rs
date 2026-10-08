use super::super::resolver::ForgeKind;
use super::*;
use crate::forge::ports::forge::{PullRequestMergeable, PullRequestTimelineItem};
use serde_json::json;

fn context() -> ForgeContext {
    ForgeContext {
        kind: ForgeKind::Gitlab,
        host: "gitlab.com".to_owned(),
        project_path: "group/team/project".to_owned(),
    }
}

fn mr() -> Value {
    json!({
        "iid": 42,
        "title": "Change",
        "web_url": "https://gitlab.com/group/team/project/-/merge_requests/42",
        "state": "opened",
        "source_branch": "feature",
        "target_branch": "main",
        "references": {
            "full": "group/team/project!42"
        },
        "detailed_merge_status": "mergeable",
        "head_pipeline": {
            "id": 7,
            "status": "running"
        }
    })
}

#[test]
fn nested_projects_are_encoded_as_one_rest_path_segment() {
    assert_eq!(
        encode_segment("group/sub group/repo"),
        "group%2Fsub%20group%2Frepo"
    );
    assert_eq!(
        mr_endpoint(&mr(), &context()).unwrap(),
        "projects/group%2Fteam%2Fproject/merge_requests/42"
    );
    assert_eq!(project_path(&json!({}), &context()), "group/team/project");
}

#[test]
fn status_preserves_merge_facts_and_pipeline_aggregate() {
    let pipeline = json!({
        "id": 8,
        "status": "success",
        "jobs": [
            {
                "id": 4,
                "name": "deploy",
                "stage": "deploy",
                "status": "manual",
                "allow_failure": true
            }
        ]
    });
    let approvals = json!({"approvals_required":2,"approved_by":[{},{}]});
    let result = status::parse(&mr(), &context(), Some(&approvals), Some(&pipeline)).unwrap();
    assert_eq!(result.forge, "gitlab");
    assert_eq!(result.project_path.as_deref(), Some("group/team/project"));
    assert_eq!(result.mergeable, PullRequestMergeable::Mergeable);
    assert_eq!(result.checks_status, "success");
    assert_eq!(result.checks[0].status, "skipped");
    assert_eq!(result.forge_specific.as_ref().unwrap()["approvalsGiven"], 2);
    assert_eq!(
        result.forge_specific.as_ref().unwrap()["pipelineStatus"],
        "running"
    );
}

#[test]
fn merged_status_preserves_source_commit_identity() {
    let mut value = mr();
    value["sha"] = json!("abc123");
    value["state"] = json!("merged");
    let result = status::parse(&value, &context(), None, None).unwrap();
    assert_eq!(result.head_sha.as_deref(), Some("abc123"));
    assert!(result.is_merged);
}

#[test]
fn old_gitlab_merge_signals_drafts_and_conflicts_are_respected() {
    let mut value = mr();
    value
        .as_object_mut()
        .unwrap()
        .remove("detailed_merge_status");
    value["merge_status"] = json!("can_be_merged");
    value["work_in_progress"] = json!(true);
    value["draft"] = Value::Null;
    value["state"] = json!("merged");
    let result = status::parse(&value, &context(), None, None).unwrap();
    assert!(result.is_merged && result.is_draft);
    assert_eq!(result.mergeable, PullRequestMergeable::Mergeable);
    value["has_conflicts"] = json!(true);
    assert_eq!(
        status::parse(&value, &context(), None, None)
            .unwrap()
            .mergeable,
        PullRequestMergeable::Conflicting
    );
    value["has_conflicts"] = json!(false);
    value["detailed_merge_status"] = json!("checking");
    assert_eq!(
        status::parse(&value, &context(), None, None)
            .unwrap()
            .mergeable,
        PullRequestMergeable::Unknown
    );
}

#[test]
fn issue_and_mr_search_use_gitlab_iids_and_full_project_paths() {
    let result = search_item(&mr(), ForgeSearchKind::ChangeRequest, &context()).unwrap();
    assert_eq!(result.number, 42);
    assert_eq!(result.state, "open");
    assert_eq!(result.forge.as_deref(), Some("gitlab"));
    let issue = json!({
        "iid": 3,
        "title": "Bug",
        "web_url": "https://gitlab.com/group/team/project/-/issues/3",
        "state": "opened",
        "labels": [
            "bug"
        ],
        "references": {
            "full": "group/team/project#3"
        }
    });
    let result = search_item(&issue, ForgeSearchKind::Issue, &context()).unwrap();
    assert_eq!(result.labels, ["bug"]);
    assert_eq!(result.project_path.as_deref(), Some("group/team/project"));
    assert!(search_item(&json!({}), ForgeSearchKind::Issue, &context()).is_err());
}

#[test]
fn checks_keep_allowed_failures_and_required_manual_jobs_distinct() {
    let value = json!({"id":9,"jobs":[
        {"id":4,"name":"allowed","stage":"test","status":"failed","allow_failure":true},
        {"id":2,"name":"required","stage":"test","status":"manual","allow_failure":false},
        {"id":3,"name":"failure","stage":"test","status":"failed"},
        {"id":1,"name":"wait","stage":"test","status":"waiting_for_resource"}]});
    let checks = pipeline::checks(&value).unwrap();
    assert_eq!(
        checks
            .iter()
            .map(|check| check.status.as_str())
            .collect::<Vec<_>>(),
        ["pending", "pending", "failure", "success"]
    );
    assert_eq!(
        checks[1].traits.as_ref().unwrap(),
        &["manual", "action_required"]
    );
    assert_eq!(checks[3].traits.as_ref().unwrap(), &["warning"]);
    for state in [
        "created",
        "waiting_for_resource",
        "preparing",
        "pending",
        "running",
        "canceling",
        "scheduled",
    ] {
        assert!(pipeline::is_active(state));
        assert_eq!(pipeline::checks_status(state), "pending");
    }
    assert_eq!(pipeline::checks_status("failed"), "failure");
    assert_eq!(pipeline::checks_status("canceled"), "none");
}

#[cfg(unix)]
mod commands;
