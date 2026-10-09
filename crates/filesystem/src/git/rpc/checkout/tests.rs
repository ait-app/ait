use serde_json::json;

use super::execute;
use crate::git::service::checkout::{
    AheadBehind, Checkout, CheckoutBranchResolution, CheckoutBranchSource,
    CheckoutBranchSuggestion, CheckoutCommit, CheckoutCommitFile, CheckoutCommitFileStatus,
    CheckoutCommits, CheckoutDiff, CheckoutDiffCompare, CheckoutFailureKind, CheckoutMergeStrategy,
    CheckoutRuntime, CheckoutRuntimeError, CheckoutStashEntry, CheckoutStatus, ParsedDiffFile,
};

#[test]
fn absent_checkouts_report_inline_failures_for_reads_and_mutations() {
    let root = tempfile::tempdir().unwrap();
    let checkout = Checkout::new(Box::new(crate::git::local::checkout::LocalCheckout::new(
        root.path().join("managed"),
    )));
    let cwd = root.path().join("missing");
    for (method, fields) in [
        ("checkout.status.get.request", json!({})),
        ("checkout.commits.list.request", json!({})),
        (
            "checkout.commits.file_diff.request",
            json!({"sha":"HEAD","path":"file.txt"}),
        ),
        (
            "checkout.branch.validate.request",
            json!({"branchName":"main"}),
        ),
        ("checkout.branch.suggestions.request", json!({})),
        ("checkout.stash.list.request", json!({})),
        ("checkout.commit.request", json!({"message":"change"})),
    ] {
        let mut params = fields;
        params["cwd"] = json!(cwd);
        let result = execute(&checkout, method, params).unwrap();
        assert!(!result["error"].is_null(), "{method}: {result}");
    }
    assert_eq!(
        execute(&checkout, "unknown", json!({})),
        Err(crate::support::error::ErrorCode::MethodNotFound)
    );
}

#[test]
fn diff_projection_preserves_highlight_tokens_on_the_wire() {
    use crate::git::ports::checkout::{DiffHunk, DiffLine, DiffLineKind, HighlightToken};
    let file = ParsedDiffFile {
        path: "source.rs".to_owned(),
        old_path: None,
        is_new: true,
        is_deleted: false,
        additions: 1,
        deletions: 0,
        status: None,
        hunks: vec![DiffHunk {
            old_start: 0,
            old_count: 0,
            new_start: 1,
            new_count: 1,
            lines: vec![DiffLine {
                kind: DiffLineKind::Add,
                content: "fn".to_owned(),
                tokens: Some(vec![HighlightToken {
                    text: "fn".to_owned(),
                    style: Some("keyword".to_owned()),
                }]),
            }],
        }],
    };
    let result = serde_json::to_value(super::protocol_diff_file(file)).unwrap();
    assert_eq!(
        result["hunks"][0]["lines"][0]["tokens"],
        json!([{"text":"fn","style":"keyword"}])
    );
}

fn highlighted_file(line_count: usize) -> ParsedDiffFile {
    use crate::git::ports::checkout::{DiffHunk, DiffLine, DiffLineKind, HighlightToken};
    let line = DiffLine {
        kind: DiffLineKind::Add,
        content: "a".repeat(20),
        tokens: Some(vec![
            HighlightToken {
                text: "a".to_owned(),
                style: Some("keyword".to_owned()),
            };
            20
        ]),
    };
    ParsedDiffFile {
        path: "source.rs".to_owned(),
        old_path: None,
        is_new: true,
        is_deleted: false,
        additions: u64::try_from(line_count).unwrap(),
        deletions: 0,
        status: None,
        hunks: vec![DiffHunk {
            old_start: 0,
            old_count: 0,
            new_start: 1,
            new_count: u64::try_from(line_count).unwrap(),
            lines: vec![line; line_count],
        }],
    }
}

#[test]
fn highlight_output_budget_keeps_large_file_diff_readable() {
    let projected = super::protocol_diff_file(highlighted_file(8000));
    assert!(
        projected.hunks[0]
            .lines
            .iter()
            .all(|line| line.tokens.is_none())
    );
    assert_eq!(projected.hunks[0].lines[0].content, "a".repeat(20));
    assert!(super::fits_diff_output_budget(&projected));
}

#[test]
fn highlight_output_budget_applies_to_the_entire_subscription_snapshot() {
    let file = highlighted_file(4000);
    assert!(
        super::protocol_diff_file(file.clone()).hunks[0].lines[0]
            .tokens
            .is_some()
    );
    let result = super::protocol_diff_result(
        "/repo",
        Ok(CheckoutDiff {
            files: vec![file.clone(), file],
            diff_too_large: false,
        }),
    );
    assert!(
        result
            .files
            .iter()
            .flat_map(|file| &file.hunks)
            .flat_map(|hunk| &hunk.lines)
            .all(|line| line.tokens.is_none())
    );
    assert!(super::fits_diff_output_budget(&result));
    assert!(result.diff_too_large.is_none());
}

fn plain_file(line_count: usize) -> ParsedDiffFile {
    let mut file = highlighted_file(1);
    file.additions = u64::try_from(line_count).unwrap();
    file.hunks[0].new_count = file.additions;
    let mut line = file.hunks[0].lines.pop().unwrap();
    line.tokens = None;
    file.hunks[0].lines = vec![line; line_count];
    file
}

#[test]
fn plain_diff_output_budget_omits_oversized_file_hunks() {
    let projected = super::protocol_diff_file(plain_file(90_000));
    assert_eq!(
        projected.status,
        Some(crate::git::protocol::checkout::ParsedDiffStatus::TooLarge)
    );
    assert!(projected.hunks.is_empty());
    assert_eq!(projected.path, "source.rs");
    assert_eq!(projected.additions, 90_000);
    assert!(super::fits_diff_output_budget(&projected));
}

#[test]
fn plain_diff_output_budget_counts_json_escaping() {
    let mut file = plain_file(1);
    file.hunks[0].lines[0].content = "\"\\\t\0".repeat(400_000);
    let projected = super::protocol_diff_file(file);
    assert_eq!(
        projected.status,
        Some(crate::git::protocol::checkout::ParsedDiffStatus::TooLarge)
    );
    assert!(projected.hunks.is_empty());
    assert!(super::fits_diff_output_budget(&projected));
}

#[test]
fn plain_diff_output_budget_applies_to_the_entire_snapshot() {
    let mut second = plain_file(50_000);
    second.path = "second.rs".to_owned();
    let result = super::protocol_diff_result(
        "/repo",
        Ok(CheckoutDiff {
            files: vec![plain_file(50_000), second],
            diff_too_large: false,
        }),
    );
    assert_eq!(result.diff_too_large, Some(true));
    assert!(result.files.is_empty());
    assert!(result.error.is_none());
    assert!(super::fits_diff_output_budget(&result));
}

#[test]
fn plain_diff_output_budget_subscription_recovers_after_oversized_update() {
    let (mut observation, _) = super::DiffObservation::prepare(
        &checkout(),
        json!({"cwd":"/repo","compare":{"mode":"uncommitted"},"subscriptionId":"large-diff"}),
    )
    .unwrap();
    let large = CheckoutDiff {
        files: vec![plain_file(50_000), plain_file(50_000)],
        diff_too_large: false,
    };
    let update = observation.update(Ok(large.clone())).unwrap().unwrap();
    assert_eq!(update["diffTooLarge"], true);
    assert_eq!(update["subscriptionId"], "large-diff");
    assert!(super::fits_diff_output_budget(&update));
    assert!(observation.update(Ok(large)).unwrap().is_none());
    let recovered = observation
        .update(Ok(CheckoutDiff {
            files: vec![plain_file(1)],
            diff_too_large: false,
        }))
        .unwrap()
        .unwrap();
    assert!(recovered.get("diffTooLarge").is_none());
    assert_eq!(
        recovered["files"][0]["hunks"][0]["lines"][0]["content"],
        "a".repeat(20)
    );
}

#[test]
fn status_projection_preserves_managed_checkout_facts() {
    let checkout = checkout();
    let result = execute(
        &checkout,
        "checkout.status.get.request",
        json!({"cwd":"/repo"}),
    )
    .unwrap();
    assert_eq!(result["isGit"], true);
    assert_eq!(result["isPaseoOwnedWorktree"], true);
    assert_eq!(result["mainRepoRoot"], "/main");
    assert_eq!(result["aheadBehind"], json!({"ahead":2,"behind":1}));
}

#[test]
fn checkout_failures_stay_inline_with_paseo_error_codes() {
    let checkout = checkout();
    let diff = execute(
        &checkout,
        "checkout.diff.get.request",
        json!({"cwd":"fail","compare":{"mode":"uncommitted"}}),
    )
    .unwrap();
    assert_eq!(diff["files"], json!([]));
    assert_eq!(diff["error"]["code"], "NOT_GIT_REPO");

    let refresh = execute(&checkout, "checkout.refresh.request", json!({"cwd":"fail"})).unwrap();
    assert_eq!(refresh["success"], false);
    assert_eq!(refresh["error"]["code"], "NOT_GIT_REPO");
}

#[test]
fn commits_project_files_and_malformed_requests_are_rejected() {
    let checkout = checkout();
    let result = execute(
        &checkout,
        "checkout.commits.list.request",
        json!({"cwd":"/repo"}),
    )
    .unwrap();
    assert_eq!(result["baseRef"], "main");
    assert_eq!(result["commits"][0]["files"][0]["status"], "modified");
    assert!(
        execute(
            &checkout,
            "checkout.commits.file_diff.request",
            json!({"cwd":"/repo","sha":"abc"}),
        )
        .is_err()
    );
}

#[test]
fn branch_queries_and_mutations_project_paseo_shapes() {
    let checkout = checkout();
    let validation = execute(
        &checkout,
        "checkout.branch.validate.request",
        json!({"cwd":"/repo","branchName":"feature"}),
    )
    .unwrap();
    assert_eq!(
        validation,
        json!({
            "exists":true,"resolvedRef":"feature","isRemote":false,"error":null
        })
    );

    let suggestions = execute(
        &checkout,
        "checkout.branch.suggestions.request",
        json!({"cwd":"/repo"}),
    )
    .unwrap();
    assert_eq!(suggestions["branches"], json!(["feature"]));
    assert_eq!(suggestions["branchDetails"][0]["hasLocal"], true);

    let switched = execute(
        &checkout,
        "checkout.branch.switch.request",
        json!({"cwd":"/repo","branch":"feature"}),
    )
    .unwrap();
    assert_eq!(switched["source"], "local");
    assert_eq!(switched["success"], true);

    let renamed = execute(
        &checkout,
        "checkout.rename_branch.request",
        json!({"cwd":"/repo","branch":"renamed"}),
    )
    .unwrap();
    assert_eq!(renamed["currentBranch"], "renamed");
}

#[test]
fn mutation_defaults_and_inline_errors_match_paseo() {
    let checkout = checkout();
    for (method, params) in [
        (
            "checkout.commit.request",
            json!({"cwd":"/repo","message":"ship"}),
        ),
        ("checkout.merge.request", json!({"cwd":"/repo"})),
        ("checkout.merge_from_base.request", json!({"cwd":"/repo"})),
        (
            "checkout.reset_workspace.request",
            json!({"cwd":"/repo","workspaceId":"workspace-1","initialBranch":"feature"}),
        ),
        ("checkout.pull.request", json!({"cwd":"/repo"})),
        ("checkout.push.request", json!({"cwd":"/repo"})),
        (
            "checkout.discard_changes.request",
            json!({"cwd":"/repo","paths":["a.txt"]}),
        ),
        ("checkout.stash.save.request", json!({"cwd":"/repo"})),
        (
            "checkout.stash.pop.request",
            json!({"cwd":"/repo","stashIndex":0}),
        ),
    ] {
        let result = execute(&checkout, method, params).unwrap();
        assert_eq!(result, json!({"cwd":"/repo","success":true,"error":null}));
    }
    let failed = execute(&checkout, "checkout.commit.request", json!({"cwd":"fail"})).unwrap();
    assert_eq!(failed["success"], false);
    assert_eq!(failed["error"]["code"], "NOT_GIT_REPO");
    assert!(
        execute(
            &checkout,
            "checkout.discard_changes.request",
            json!({"cwd":"/repo","paths":[]}),
        )
        .is_err()
    );
}

#[test]
fn stash_list_defaults_to_paseo_only_and_preserves_entry_shape() {
    let result = execute(
        &checkout(),
        "checkout.stash.list.request",
        json!({"cwd":"/repo"}),
    )
    .unwrap();
    assert_eq!(result["entries"][0]["branch"], "feature");
    assert_eq!(result["entries"][0]["isPaseo"], true);
}

fn checkout() -> Checkout {
    Checkout::new(Box::new(FakeCheckout))
}

#[derive(Debug)]
struct FakeCheckout;

impl CheckoutRuntime for FakeCheckout {
    fn status(&self, cwd: &str) -> Result<CheckoutStatus, CheckoutRuntimeError> {
        fail(cwd)?;
        Ok(CheckoutStatus {
            is_git: true,
            repo_root: Some("/repo".to_owned()),
            main_repo_root: Some("/main".to_owned()),
            current_branch: Some("feature".to_owned()),
            is_dirty: Some(true),
            branch_status: None,
            base_ref: Some("main".to_owned()),
            ahead_behind: Some(AheadBehind {
                ahead: 2,
                behind: 1,
            }),
            upstream_ref: Some("refs/remotes/origin/feature".to_owned()),
            ahead_of_origin: Some(1),
            behind_of_origin: Some(0),
            has_remote: true,
            remote_url: Some("git@example.test:org/repo.git".to_owned()),
            is_managed_worktree: true,
        })
    }

    fn refresh(&self, cwd: &str) -> Result<(), CheckoutRuntimeError> {
        fail(cwd)
    }

    fn diff(
        &self,
        cwd: &str,
        _compare: &CheckoutDiffCompare,
    ) -> Result<CheckoutDiff, CheckoutRuntimeError> {
        fail(cwd)?;
        Ok(CheckoutDiff {
            files: Vec::new(),
            diff_too_large: false,
        })
    }

    fn commits(&self, cwd: &str) -> Result<CheckoutCommits, CheckoutRuntimeError> {
        fail(cwd)?;
        Ok(CheckoutCommits {
            base_ref: Some("main".to_owned()),
            commits: vec![CheckoutCommit {
                sha: "1".repeat(40),
                short_sha: "1111111".to_owned(),
                subject: "subject".to_owned(),
                author_name: "Author".to_owned(),
                author_date: "2026-01-01T00:00:00Z".to_owned(),
                is_on_remote: false,
                is_on_base: false,
                files: vec![CheckoutCommitFile {
                    path: "src/main.rs".to_owned(),
                    additions: 1,
                    deletions: 1,
                    status: Some(CheckoutCommitFileStatus::Modified),
                }],
            }],
        })
    }

    fn commit_file_diff(
        &self,
        cwd: &str,
        _sha: &str,
        _path: &str,
    ) -> Result<Option<ParsedDiffFile>, CheckoutRuntimeError> {
        fail(cwd)?;
        Ok(None)
    }

    fn validate_branch(
        &self,
        cwd: &str,
        branch: &str,
    ) -> Result<CheckoutBranchResolution, CheckoutRuntimeError> {
        fail(cwd)?;
        Ok(CheckoutBranchResolution::Local(branch.to_owned()))
    }

    fn branch_suggestions(
        &self,
        cwd: &str,
        _query: Option<&str>,
        _limit: usize,
    ) -> Result<Vec<CheckoutBranchSuggestion>, CheckoutRuntimeError> {
        fail(cwd)?;
        Ok(vec![CheckoutBranchSuggestion {
            name: "feature".to_owned(),
            committer_date: 1,
            has_local: true,
            has_remote: false,
            local_ahead: None,
            local_behind: None,
        }])
    }

    fn switch_branch(
        &self,
        cwd: &str,
        _branch: &str,
    ) -> Result<CheckoutBranchSource, CheckoutRuntimeError> {
        fail(cwd)?;
        Ok(CheckoutBranchSource::Local)
    }

    fn rename_branch(&self, cwd: &str, branch: &str) -> Result<String, CheckoutRuntimeError> {
        fail(cwd)?;
        Ok(branch.to_owned())
    }

    fn commit(
        &self,
        cwd: &str,
        _message: &str,
        _add_all: bool,
    ) -> Result<(), CheckoutRuntimeError> {
        fail(cwd)
    }

    fn merge_to_base(
        &self,
        cwd: &str,
        _base_ref: Option<&str>,
        _strategy: CheckoutMergeStrategy,
        _require_clean_target: bool,
    ) -> Result<(), CheckoutRuntimeError> {
        fail(cwd)
    }

    fn merge_from_base(
        &self,
        cwd: &str,
        _base_ref: Option<&str>,
        _require_clean_target: bool,
    ) -> Result<(), CheckoutRuntimeError> {
        fail(cwd)
    }

    fn reset_workspace(
        &self,
        cwd: &str,
        _initial_branch: &str,
    ) -> Result<(), CheckoutRuntimeError> {
        fail(cwd)
    }

    fn pull(&self, cwd: &str) -> Result<(), CheckoutRuntimeError> {
        fail(cwd)
    }

    fn push(&self, cwd: &str) -> Result<(), CheckoutRuntimeError> {
        fail(cwd)
    }

    fn discard_changes(&self, cwd: &str, _paths: &[String]) -> Result<(), CheckoutRuntimeError> {
        fail(cwd)
    }

    fn stash_save(&self, cwd: &str, _branch: Option<&str>) -> Result<(), CheckoutRuntimeError> {
        fail(cwd)
    }

    fn stash_pop(&self, cwd: &str, _index: usize) -> Result<(), CheckoutRuntimeError> {
        fail(cwd)
    }

    fn stashes(
        &self,
        cwd: &str,
        _paseo_only: bool,
    ) -> Result<Vec<CheckoutStashEntry>, CheckoutRuntimeError> {
        fail(cwd)?;
        Ok(vec![CheckoutStashEntry {
            index: 0,
            message: "paseo-auto-stash: feature".to_owned(),
            branch: Some("feature".to_owned()),
            is_paseo: true,
        }])
    }
}

fn fail(cwd: &str) -> Result<(), CheckoutRuntimeError> {
    if cwd == "fail" {
        Err(CheckoutRuntimeError {
            kind: CheckoutFailureKind::NotGitRepository,
            message: "Not a git repository".to_owned(),
        })
    } else {
        Ok(())
    }
}

#[test]
fn diff_observation_suppresses_duplicates_and_preserves_inline_errors() {
    let checkout = checkout();
    let (mut observation, initial) = super::DiffObservation::prepare(
        &checkout,
        json!({"cwd":"/repo","compare":{"mode":"uncommitted"},"subscriptionId":"diff-one"}),
    )
    .unwrap();
    assert_eq!(initial["subscriptionId"], "diff-one");
    assert_eq!(observation.id(), "diff-one");
    assert_eq!(observation.cwd(), "/repo");
    assert!(
        observation
            .update(checkout.diff(observation.cwd(), observation.compare()))
            .unwrap()
            .is_none()
    );
    let failed = checkout.diff("fail", observation.compare());
    let changed = observation.update(failed).unwrap().unwrap();
    assert_eq!(changed["error"]["code"], "NOT_GIT_REPO");
    assert!(
        observation
            .update(checkout.diff("fail", observation.compare()))
            .unwrap()
            .is_none()
    );
    assert!(
        super::DiffObservation::prepare(
            &checkout,
            json!({"cwd":"/repo","compare":{"mode":"uncommitted"},"subscriptionId":""})
        )
        .is_err()
    );
}
