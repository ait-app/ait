use serde_json::{Value, json};

mod failures;

use super::execute;
use crate::forge::service::forge::{
    CheckDetails, Forge, ForgeAuthState, ForgeRuntime, ForgeRuntimeError, ForgeSearch,
    ForgeSearchItem, ForgeSearchKind, PullRequestCreated, PullRequestMergeMethod,
    PullRequestMergeable, PullRequestStatus, PullRequestStatusRead, PullRequestTimeline,
    PullRequestTimelineItem, TimelineReviewState,
};

#[test]
fn timeline_projection_preserves_failure_categories_and_inline_comment_locations() {
    use crate::forge::ports::forge::{TimelineCommentLocation, TimelineError, TimelineErrorKind};
    for (kind, expected) in [
        (TimelineErrorKind::NotFound, "not_found"),
        (TimelineErrorKind::Forbidden, "forbidden"),
        (TimelineErrorKind::Unknown, "unknown"),
    ] {
        let error = super::protocol_timeline_error(TimelineError {
            kind,
            message: "request failed".to_owned(),
        });
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            json!({"kind":expected,"message":"request failed"})
        );
    }
    let comment = PullRequestTimelineItem::Comment {
        id: "comment".to_owned(),
        author: "reviewer".to_owned(),
        author_url: None,
        avatar_url: None,
        body: "Fix this".to_owned(),
        created_at: 1,
        url: "https://example/comment".to_owned(),
        review_id: Some("review".to_owned()),
        thread_id: Some("thread".to_owned()),
        thread_is_resolved: Some(false),
        location: Some(TimelineCommentLocation {
            path: "src/lib.rs".to_owned(),
            line: Some(12),
            start_line: Some(10),
            thread_id: Some("thread".to_owned()),
            is_resolved: Some(false),
            is_outdated: Some(true),
        }),
    };
    let value = serde_json::to_value(super::protocol_timeline_item(comment)).unwrap();
    assert_eq!(
        value["location"],
        json!({"path":"src/lib.rs","line":12,"startLine":10,"threadId":"thread","isResolved":false,"isOutdated":true})
    );
}

#[derive(Debug)]
struct FakeForge;

impl ForgeRuntime for FakeForge {
    fn search(
        &self,
        _cwd: &str,
        _query: &str,
        _limit: usize,
        _kinds: &[ForgeSearchKind],
    ) -> Result<ForgeSearch, ForgeRuntimeError> {
        Ok(ForgeSearch {
            items: vec![ForgeSearchItem {
                kind: ForgeSearchKind::ChangeRequest,
                forge: Some("github".to_owned()),
                number: 4,
                title: "Fix".to_owned(),
                url: "https://github.com/acme/app/pull/4".to_owned(),
                state: "open".to_owned(),
                body: None,
                labels: vec!["bug".to_owned()],
                project_path: Some("acme/app".to_owned()),
                base_ref_name: Some("main".to_owned()),
                head_ref_name: Some("fix".to_owned()),
                updated_at: Some("2026-01-01T00:00:00Z".to_owned()),
            }],
            auth_state: ForgeAuthState::Authenticated,
        })
    }

    fn create_pull_request(
        &self,
        _cwd: &str,
        _title: &str,
        _body: &str,
        _base_ref: Option<&str>,
    ) -> Result<PullRequestCreated, ForgeRuntimeError> {
        Ok(PullRequestCreated {
            url: "https://github.com/acme/app/pull/4".to_owned(),
            number: 4,
        })
    }

    fn current_pull_request_status(
        &self,
        _cwd: &str,
    ) -> Result<PullRequestStatusRead, ForgeRuntimeError> {
        Ok(PullRequestStatusRead {
            status: Some(PullRequestStatus {
                forge: "github".to_owned(),
                project_path: Some("acme/app".to_owned()),
                number: Some(4),
                url: "https://github.com/acme/app/pull/4".to_owned(),
                title: "Fix".to_owned(),
                state: "open".to_owned(),
                base_ref_name: "main".to_owned(),
                head_ref_name: "fix".to_owned(),
                head_sha: None,
                is_merged: false,
                is_draft: false,
                mergeable: PullRequestMergeable::Mergeable,
                checks: Vec::new(),
                checks_status: "none".to_owned(),
                review_decision: None,
                repo_owner: Some("acme".to_owned()),
                repo_name: Some("app".to_owned()),
                github: None,
                forge_specific: None,
            }),
            auth_state: ForgeAuthState::Authenticated,
            forge: Some("github".to_owned()),
        })
    }

    fn merge_current_pull_request(
        &self,
        _cwd: &str,
        _merge_method: PullRequestMergeMethod,
    ) -> Result<(), ForgeRuntimeError> {
        Ok(())
    }

    fn set_current_pull_request_auto_merge(
        &self,
        _cwd: &str,
        _enabled: bool,
        _merge_method: Option<PullRequestMergeMethod>,
    ) -> Result<(), ForgeRuntimeError> {
        Ok(())
    }

    fn pull_request_timeline(
        &self,
        _cwd: &str,
        pr_number: u64,
        _repo_owner: &str,
        _repo_name: &str,
    ) -> Result<PullRequestTimeline, ForgeRuntimeError> {
        Ok(PullRequestTimeline {
            pr_number,
            items: vec![PullRequestTimelineItem::Review {
                id: "R1".to_owned(),
                author: "octo".to_owned(),
                author_url: None,
                avatar_url: None,
                body: "ship".to_owned(),
                created_at: 1,
                url: "https://example/review".to_owned(),
                review_state: TimelineReviewState::Approved,
            }],
            truncated: false,
            error: None,
            auth_state: ForgeAuthState::Authenticated,
        })
    }

    fn check_details(
        &self,
        _cwd: &str,
        query: crate::forge::ports::forge::CheckDetailsQuery<'_>,
    ) -> Result<CheckDetails, ForgeRuntimeError> {
        Ok(CheckDetails {
            check_run_id: query.check_run_id.unwrap(),
            workflow_run_id: query.workflow_run_id,
            name: "tests".to_owned(),
            status: Some("completed".to_owned()),
            conclusion: Some("success".to_owned()),
            url: None,
            details_url: None,
            output: Some(crate::forge::ports::forge::CheckOutput {
                title: Some("Failure".to_owned()),
                summary: Some("One job failed".to_owned()),
                text: Some("Details".to_owned()),
            }),
            annotations: Vec::new(),
            failed_jobs: vec![crate::forge::ports::forge::CheckFailedJob {
                job_id: 70,
                name: "linux".to_owned(),
                status: Some("completed".to_owned()),
                conclusion: Some("failure".to_owned()),
                url: Some("https://example/job/70".to_owned()),
                log_tail: Some("assertion failed".to_owned()),
                log_truncated: Some(true),
            }],
            truncated: false,
            pipeline: None,
        })
    }
}

fn forge() -> Forge {
    Forge::new(Box::new(FakeForge))
}

#[test]
fn projects_neutral_and_github_search_shapes() {
    let neutral = execute(
        &forge(),
        "forge.search.request",
        json!({"cwd":"/repo","query":"fix","kinds":["github-pr"]}),
    )
    .unwrap();
    assert_eq!(neutral["items"][0]["kind"], "change_request");
    assert_eq!(neutral["authState"], "authenticated");
    let legacy = execute(
        &forge(),
        "github.search.request",
        json!({"cwd":"/repo","query":"fix"}),
    )
    .unwrap();
    assert_eq!(legacy["items"][0]["kind"], "pr");
    assert_eq!(legacy["featuresEnabled"], true);
    assert_eq!(legacy["githubFeaturesEnabled"], true);
}

#[test]
fn serves_pr_mutations_status_timeline_and_check_details() {
    let created = execute(
        &forge(),
        "checkout.pr.create.request",
        json!({"cwd":"/repo","title":"Fix","body":"Details"}),
    )
    .unwrap();
    assert_eq!(created["number"], 4);
    assert!(created["error"].is_null());
    let merged = execute(
        &forge(),
        "checkout.pr.merge.request",
        json!({"cwd":"/repo","mergeMethod":"squash"}),
    )
    .unwrap();
    assert_eq!(merged["success"], true);
    let auto = execute(
        &forge(),
        "checkout.forge.set_auto_merge.request",
        json!({"cwd":"/repo","enabled":true,"mergeMethod":"rebase"}),
    )
    .unwrap();
    assert_eq!(auto["enabled"], true);
    let status = execute(
        &forge(),
        "checkout.pr.status.request",
        json!({"cwd":"/repo"}),
    )
    .unwrap();
    assert_eq!(status["status"]["number"], 4);
    let timeline = execute(
        &forge(),
        "checkout.pr.timeline.request",
        json!({"cwd":"/repo","prNumber":4,"repoOwner":"acme","repoName":"app"}),
    )
    .unwrap();
    assert_eq!(timeline["items"][0]["reviewState"], "approved");
    let details = execute(
        &forge(),
        "checkout.github.get_check_details.request",
        json!({"cwd":"/repo","repoOwner":"acme","repoName":"app","checkRunId":9}),
    )
    .unwrap();
    assert_eq!(details["details"]["checkRunId"], 9);
    assert_eq!(
        details["details"]["output"],
        json!({"title":"Failure","summary":"One job failed","text":"Details"})
    );
    assert_eq!(
        details["details"]["failedJobs"][0],
        json!({"jobId":70,"name":"linux","status":"completed","conclusion":"failure","url":"https://example/job/70","logTail":"assertion failed","logTruncated":true})
    );
}

#[test]
fn keeps_paseo_inline_errors_for_invalid_application_shapes() {
    let created = execute(
        &forge(),
        "checkout.pr.create.request",
        json!({"cwd":"/repo"}),
    )
    .unwrap();
    assert_eq!(created["url"], Value::Null);
    assert_eq!(created["error"]["code"], "UNKNOWN");
    let auto = execute(
        &forge(),
        "checkout.github.set_auto_merge.request",
        json!({"cwd":"/repo","enabled":true}),
    )
    .unwrap();
    assert_eq!(auto["success"], false);
    assert!(
        auto["error"]["message"]
            .as_str()
            .unwrap()
            .contains("mergeMethod")
    );
    let timeline = execute(
        &forge(),
        "checkout.pr.timeline.request",
        json!({"cwd":"/repo","prNumber":0,"repoOwner":"bad/owner","repoName":"app"}),
    )
    .unwrap();
    assert_eq!(timeline["error"]["kind"], "unknown");
}
