//! Offline glab subprocess regression scenarios.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use domain::workspace::protocol::workspace::ChecksStatus;

use super::*;
use crate::forge::ports::{CheckDetailsQuery, ForgeRuntime};
use domain::forge::AuthorAccess;
use model::workspace::worktrees::ChangeRequestResolver;

struct Fixture {
    root: tempfile::TempDir,
    repository: PathBuf,
    forge: LocalForge,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("repo");
        fs::create_dir(&repository).unwrap();
        let forge = LocalForge::with_executable(root.path().join("gh"));
        let fixture = Self {
            root,
            repository,
            forge,
        };
        fixture.git(&["init", "-b", "main"]);
        fixture.git(&["config", "user.name", "Test"]);
        fixture.git(&["config", "user.email", "test@example.invalid"]);
        fixture.git(&["commit", "--allow-empty", "-m", "initial"]);
        fixture.git(&["switch", "-c", "feature"]);
        fixture.git(&[
            "remote",
            "add",
            "origin",
            "git@gitlab.com:group/team/project.git",
        ]);
        fixture.script("gh", "echo \"$*\" >> \"$root/gh-calls\"\nexit 1");
        fixture.script("ssh", "printf 'hostname gitlab.com\\n'");
        fixture.script(
            "glab",
            r#"
printf '%s\n' "$@" >> "$root/calls"
printf '%s|%s\n' "$GITLAB_HOST" "$GIT_TERMINAL_PROMPT" >> "$root/env"
if [ -f "$root/error-$1-$2" ]; then cat "$root/error-$1-$2" >&2; exit 1; fi
case "$1 $2" in
  'auth status') exit 0 ;;
  'mr list') cat "$root/list.json" ;;
  'mr view') cat "$root/mr.json" ;;
  'issue list') cat "$root/issues.json" ;;
  'ci get') cat "$root/pipeline.json" ;;
  'mr create') printf 'Created\nhttps://gitlab.com/group/team/project/-/merge_requests/42\n' ;;
  'mr merge') echo merge >> "$root/mutations" ;;
  'api --method') echo cancel >> "$root/mutations" ;;
  api*)
    case "$2" in
      */approvals) cat "$root/approvals.json" ;;
      user) cat "$root/user.json" ;;
      *members/all*) cat "$root/members.json" ;;
      *notes*) cat "$root/notes.json" ;;
      *discussions*page=101) cat "$root/next.json" ;;
      *discussions*) cat "$root/discussions.json" ;;
      *) echo 'unexpected api call' >&2; exit 2 ;;
    esac ;;
  *) echo 'unexpected glab call' >&2; exit 2 ;;
esac
"#,
        );
        fixture.payload("mr", &mr());
        fixture.payload("list", &json!([mr()]));
        fixture.payload("issues", &json!([]));
        fixture.payload(
            "approvals",
            &json!({"approvals_required":1,"approved_by":[{}]}),
        );
        fixture.payload(
            "pipeline",
            &json!({
                "id": 7,
                "status": "success",
                "ref": "feature",
                "jobs": [
                    {
                        "id": 8,
                        "name": "test",
                        "stage": "test",
                        "status": "success"
                    }
                ]
            }),
        );
        fixture.payload("discussions", &json!([]));
        fixture.payload("user", &json!({"username": "owner"}));
        fixture.payload("notes", &json!([]));
        fixture.payload("members", &json!([]));
        fixture.payload("next", &json!([]));
        fixture
    }

    fn cwd(&self) -> &str {
        self.repository.to_str().unwrap()
    }

    fn script(&self, name: &str, body: &str) {
        let path = self.root.path().join(name);
        fs::write(&path, format!("#!/bin/sh\nroot=${{0%/*}}\n{body}\n")).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }

    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.repository)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn payload(&self, name: &str, value: &Value) {
        fs::write(
            self.root.path().join(format!("{name}.json")),
            value.to_string(),
        )
        .unwrap();
    }
    fn calls(&self) -> String {
        fs::read_to_string(self.root.path().join("calls")).unwrap_or_default()
    }
    fn error(&self, operation: &str, message: &str) {
        fs::write(self.root.path().join(format!("error-{operation}")), message).unwrap();
    }
    fn status(&self) -> PullRequestStatusRead {
        self.forge.current_pull_request_status(self.cwd()).unwrap()
    }
}

#[test]
fn gitlab_cloud_never_invokes_github_and_retains_its_identity_without_an_mr() {
    let fixture = Fixture::new();
    let read = fixture.status();
    assert_eq!(read.forge.as_deref(), Some("gitlab"));
    assert_eq!(read.status.unwrap().checks_status, ChecksStatus::Success);
    assert!(!fixture.root.path().join("gh-calls").exists());
    assert!(
        fixture
            .calls()
            .contains("--repo\nhttps://gitlab.com/group/team/project\n")
    );
    fixture.payload("list", &json!([]));
    let read = fixture.status();
    assert!(read.status.is_none());
    assert_eq!(read.auth_state, ForgeAuthState::Authenticated);
    assert_eq!(read.forge.as_deref(), Some("gitlab"));
}

#[test]
fn self_managed_hosts_are_probed_once_and_rest_requests_use_the_detected_host() {
    let fixture = Fixture::new();
    fixture.git(&[
        "remote",
        "set-url",
        "origin",
        "https://code.company.test/group/team/project.git",
    ]);
    for _ in 0..2 {
        assert_eq!(fixture.status().forge.as_deref(), Some("gitlab"));
    }
    assert_eq!(
        fixture
            .calls()
            .matches("auth\nstatus\n--hostname\ncode.company.test\n")
            .count(),
        1
    );
    assert!(fixture.calls().contains("--hostname\ncode.company.test\n"));
    assert!(
        fixture
            .calls()
            .contains("--repo\nhttps://code.company.test/group/team/project\n")
    );
    assert_eq!(
        fs::read_to_string(fixture.root.path().join("gh-calls"))
            .unwrap()
            .lines()
            .count(),
        1
    );
}

#[test]
fn ssh_aliases_are_resolved_before_selecting_the_forge() {
    let fixture = Fixture::new();
    fixture.git(&[
        "remote",
        "set-url",
        "origin",
        "git@gitlab-work:group/team/project.git",
    ]);
    assert_eq!(fixture.status().forge.as_deref(), Some("gitlab"));
    assert!(!fixture.root.path().join("gh-calls").exists());
}

#[test]
fn gitlab_checkout_resolves_refs_and_fork_trust_without_querying_github() {
    let fixture = Fixture::new();
    let result = fixture.forge.resolve_change_request(
        fixture.cwd(),
        &domain::workspace::worktrees::WorktreeChangeRequest {
            forge: None,
            number: 42,
            project_path: None,
        },
        None,
    );
    let target = result.unwrap();
    assert_eq!(
        target.checkout_refs[0].reference,
        "refs/merge-requests/42/head"
    );
    assert_eq!(target.checkout_refs[1].reference, "refs/heads/feature");
    assert!(target.track_origin && target.untrusted_repository.is_none());
    let source = domain::workspace::worktrees::WorktreeChangeRequest {
        forge: Some("gitlab".to_owned()),
        number: 42,
        project_path: None,
    };
    let mut value = mr();
    value["source_project_id"] = json!(12);
    value["target_project_id"] = json!(13);
    fixture.payload("mr", &value);
    let fork = fixture
        .forge
        .resolve_change_request(fixture.cwd(), &source, Some("review"))
        .unwrap();
    assert!(!fork.track_origin && fork.push_remote_url.is_none());
    assert_eq!(fork.local_branch, "review");
    assert_eq!(
        fork.untrusted_repository.as_deref(),
        Some("GitLab project 12")
    );
    value["iid"] = json!(99);
    fixture.payload("mr", &value);
    assert!(
        fixture
            .forge
            .resolve_change_request(fixture.cwd(), &source, None)
            .is_err()
    );
    fixture.error("mr-view", "HTTP 404 not found");
    assert!(
        fixture
            .forge
            .resolve_change_request(fixture.cwd(), &source, None)
            .is_err()
    );
    assert!(!fixture.root.path().join("gh-calls").exists());
}

#[test]
fn unknown_hosts_do_not_default_to_github_and_negative_probes_are_bounded() {
    let fixture = Fixture::new();
    fixture.git(&[
        "remote",
        "set-url",
        "origin",
        "https://unknown.test/team/project.git",
    ]);
    fixture.error("auth-status", "not logged in");
    for _ in 0..2 {
        assert_eq!(fixture.status().auth_state, ForgeAuthState::NoRemote);
    }
    assert_eq!(fixture.calls().matches("auth\nstatus").count(), 1);
    assert!(!fixture.calls().contains("mr\nlist"));
    assert!(
        fixture
            .forge
            .resolve_change_request(
                fixture.cwd(),
                &domain::workspace::worktrees::WorktreeChangeRequest {
                    forge: None,
                    number: 42,
                    project_path: None
                },
                None
            )
            .is_err()
    );
    assert!(
        !fs::read_to_string(fixture.root.path().join("gh-calls"))
            .unwrap()
            .contains("repo view")
    );
}

#[test]
fn missing_cli_and_expired_login_are_gitlab_setup_states() {
    let fixture = Fixture::new();
    fixture.error("mr-list", "authentication required; glab auth login");
    assert_eq!(fixture.status().auth_state, ForgeAuthState::Unauthenticated);
    assert_eq!(fixture.status().forge.as_deref(), Some("gitlab"));
    fs::remove_file(fixture.root.path().join("glab")).unwrap();
    assert_eq!(fixture.status().auth_state, ForgeAuthState::CliMissing);
    assert_eq!(fixture.status().forge.as_deref(), Some("gitlab"));
}

#[test]
fn branch_reuse_does_not_revive_a_stale_merged_request() {
    let fixture = Fixture::new();
    let mut closed = mr();
    closed["state"] = json!("merged");
    closed["sha"] = json!("outdated");
    fixture.payload("list", &json!([closed]));
    assert!(fixture.status().status.is_none());
    closed["sha"] = json!(fixture.git(&["rev-parse", "HEAD"]));
    fixture.payload("list", &json!([closed]));
    fixture.payload("mr", &closed);
    assert!(fixture.status().status.unwrap().is_merged);
    fixture.git(&["checkout", "--detach"]);
    assert!(fixture.status().status.is_none());
}

#[test]
fn search_uses_issue_output_and_mr_json_flags_with_literal_user_arguments() {
    let fixture = Fixture::new();
    fixture.payload(
        "issues",
        &json!([
            {
                "iid": 1,
                "title": "Bug",
                "web_url": "https://gitlab.com/group/team/project/-/issues/1",
                "state": "opened",
                "updated_at": "2026-09-29",
                "author": {
                    "username": "dev",
                    "avatar_url": "https://gitlab.com/uploads/dev.png"
                }
            }
        ]),
    );
    let result = fixture
        .forge
        .search(
            fixture.cwd(),
            "--not-an-option; echo test",
            10,
            &[ForgeSearchKind::Issue, ForgeSearchKind::ChangeRequest],
            ForgeSearchStates::Open,
        )
        .unwrap();
    assert_eq!(result.items.len(), 2);
    let issue = result
        .items
        .iter()
        .find(|item| item.kind == ForgeSearchKind::Issue)
        .unwrap();
    assert_eq!(issue.author.as_deref(), Some("dev"));
    assert_eq!(
        issue.author_avatar_url.as_deref(),
        Some("https://gitlab.com/uploads/dev.png")
    );
    assert!(fixture.calls().contains("issue\nlist\n-O\njson\n"));
    assert!(fixture.calls().contains("mr\nlist\n-F\njson\n"));
    assert!(
        fixture
            .calls()
            .contains("--search\n--not-an-option; echo test\n")
    );
    assert!(!fixture.calls().contains("--all\n"));
    fixture
        .forge
        .search(
            fixture.cwd(),
            "7",
            10,
            &[ForgeSearchKind::ChangeRequest],
            ForgeSearchStates::All,
        )
        .unwrap();
    assert!(fixture.calls().contains("--search\n7\n--all\n"));
    fixture.error("issue-list", "HTTP 403 forbidden");
    assert!(
        fixture
            .forge
            .search(
                fixture.cwd(),
                "",
                10,
                &[ForgeSearchKind::Issue],
                ForgeSearchStates::Open
            )
            .is_err()
    );
}

#[test]
fn create_pushes_to_the_configured_remote_and_returns_the_mr_iid() {
    let fixture = Fixture::new();
    let remote = fixture.root.path().join("remote.git");
    fixture.git(&["init", "--bare", remote.to_str().unwrap()]);
    fixture.git(&[
        "config",
        &format!("url.{}.insteadOf", remote.display()),
        "git@gitlab.com:group/team/project.git",
    ]);
    let created = fixture
        .forge
        .create_pull_request(fixture.cwd(), "Title", "Details", Some("main"))
        .unwrap();
    assert_eq!(created.number, 42);
    assert!(fixture.calls().contains(
        "mr\ncreate\n--title\nTitle\n--description\nDetails\n\
         --source-branch\nfeature\n--target-branch\nmain\n--yes\n"
    ));
}

#[test]
fn merge_and_auto_merge_match_paseo_scheduling_guards() {
    let fixture = Fixture::new();
    fixture
        .forge
        .merge_current_pull_request(fixture.cwd(), PullRequestMergeMethod::Squash)
        .unwrap();
    assert!(
        fixture
            .calls()
            .contains("mr\nmerge\n42\n--auto-merge=false\n--yes\n--squash\n")
    );
    fixture
        .forge
        .set_current_pull_request_auto_merge(
            fixture.cwd(),
            true,
            Some(PullRequestMergeMethod::Rebase),
        )
        .unwrap();
    assert!(
        fixture
            .calls()
            .contains("mr\nmerge\n42\n--auto-merge\n--yes\n--rebase\n")
    );
    fixture
        .forge
        .set_current_pull_request_auto_merge(fixture.cwd(), false, None)
        .unwrap();
    assert!(fixture.calls().contains(
        "api\n--method\nPOST\nprojects/group%2Fteam%2Fproject/\
         merge_requests/42/cancel_merge_when_pipeline_succeeds\n"
    ));
    let mut value = mr();
    value["head_pipeline"]["status"] = json!("success");
    fixture.payload("mr", &value);
    assert!(
        fixture
            .forge
            .set_current_pull_request_auto_merge(
                fixture.cwd(),
                true,
                Some(PullRequestMergeMethod::Merge)
            )
            .is_err()
    );
    value["merge_when_pipeline_succeeds"] = json!(true);
    fixture.payload("mr", &value);
    assert!(
        fixture
            .forge
            .merge_current_pull_request(fixture.cwd(), PullRequestMergeMethod::Merge)
            .is_err()
    );
    assert_eq!(
        fs::read_to_string(fixture.root.path().join("mutations"))
            .unwrap()
            .lines()
            .count(),
        3
    );
}

#[test]
fn optional_pipeline_and_approval_errors_do_not_hide_the_mr() {
    let fixture = Fixture::new();
    fixture.error("ci-get", "HTTP 404 pipeline not found");
    fixture.payload("approvals", &json!(null));
    let status = fixture.status().status.unwrap();
    assert!(status.checks.is_empty());
    assert_eq!(status.checks_status, ChecksStatus::Pending);
    fixture.error("ci-get", "HTTP 401 authentication failed");
    assert_eq!(fixture.status().auth_state, ForgeAuthState::Unauthenticated);
}

#[test]
fn pipeline_details_address_fork_mrs_by_iid_and_preserve_stage_order() {
    let fixture = Fixture::new();
    fixture.payload(
        "pipeline",
        &json!({
            "id": 19,
            "status": "success",
            "ref": "feature",
            "jobs": [
                {
                    "id": 3,
                    "name": "deploy",
                    "stage": "deploy",
                    "status": "manual",
                    "allow_failure": true
                },
                {
                    "id": 2,
                    "name": "test",
                    "stage": "test",
                    "status": "failed",
                    "allow_failure": true
                },
                {
                    "id": 1,
                    "name": "build",
                    "stage": "build",
                    "status": "success"
                }
            ]
        }),
    );
    let details = fixture
        .forge
        .check_details(
            fixture.cwd(),
            CheckDetailsQuery {
                check_run_id: Some(9),
                change_request_number: Some(42),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(details.check_run_id, 19);
    let pipeline = details.pipeline.unwrap();
    assert_eq!(pipeline["stages"][0]["name"], "build");
    assert_eq!(pipeline["stages"][1]["status"], "success");
    assert_eq!(pipeline["stages"][2]["status"], "success");
    assert!(fixture.calls().contains("--merge-request\n42\n"));
    fixture
        .forge
        .check_details(
            fixture.cwd(),
            CheckDetailsQuery {
                check_run_id: Some(19),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(fixture.calls().contains("--pipeline-id\n19\n"));
}

#[test]
fn timeline_keeps_human_threads_inline_locations_and_exact_pagination() {
    let fixture = Fixture::new();
    fixture.payload(
        "discussions",
        &json!([
            {"id":"general", "individual_note":false, "notes":[{
                "id":3, "body":"reply", "created_at":"2026-09-29T01:00:00Z",
                "resolvable":true, "resolved":true
            }]},
            {"id":"file", "individual_note":false, "notes":[{"id":1, "system":true}, {
                "id":2, "body":"inline", "created_at":"2026-09-28T01:00:00Z",
                "position":{"new_path":"src/lib.rs", "new_line":5,
                    "line_range":{"start":{"new_line":3}}},
                "resolvable":true, "resolved":false
            }]}
        ]),
    );
    let result = fixture
        .forge
        .pull_request_timeline(fixture.cwd(), 42, "group", "project")
        .unwrap();
    assert_eq!(result.items.len(), 2);
    let PullRequestTimelineItem::Comment {
        location: Some(location),
        ..
    } = &result.items[0]
    else {
        panic!("expected inline comment");
    };
    assert_eq!(location.start_line, Some(3));
    assert_eq!(location.line, Some(5));
    assert_eq!(location.is_resolved, Some(false));
    assert!(matches!(
        &result.items[1],
        PullRequestTimelineItem::Comment {
            thread_is_resolved: Some(true),
            ..
        }
    ));
    fixture.payload(
        "discussions",
        &json!(vec![json!({"id":"empty","notes":[]}); 100]),
    );
    assert!(
        !fixture
            .forge
            .pull_request_timeline(fixture.cwd(), 42, "group", "project")
            .unwrap()
            .truncated
    );
    fixture.payload("next", &json!([{"id":"later","notes":[]}]));
    assert!(
        fixture
            .forge
            .pull_request_timeline(fixture.cwd(), 42, "group", "project")
            .unwrap()
            .truncated
    );
    fixture.error("mr-view", "HTTP 404 merge request not found");
    assert!(
        fixture
            .forge
            .pull_request_timeline(fixture.cwd(), 42, "group", "project")
            .unwrap()
            .error
            .is_some()
    );
}

#[test]
fn reset_detaches_existing_mrs_and_allows_new_mrs_on_the_same_branch() {
    let fixture = Fixture::new();
    fixture.git(&[
        "config",
        "branch.feature.ait-reset-at",
        "2026-10-13T00:00:00Z",
    ]);
    for (created_at, expected) in [
        (Some("2026-10-12T00:00:00Z"), false),
        (Some("2026-10-13T00:00:00Z"), false),
        (Some("2026-10-14T00:00:00Z"), true),
        (None, false),
    ] {
        let mut request = mr();
        request["created_at"] = json!(created_at);
        fixture.payload("list", &json!([request]));
        assert_eq!(
            fixture.status().status.is_some(),
            expected,
            "{created_at:?}"
        );
    }
    fixture.git(&["switch", "-c", "another-workspace"]);
    let mut request = mr();
    request["source_branch"] = json!("another-workspace");
    fixture.payload("list", &json!([request]));
    assert!(fixture.status().status.is_some());
}

#[test]
fn a_merge_request_read_by_number_projects_jobs_and_marks_the_signed_in_users_notes() {
    let fixture = Fixture::new();
    let mut merged = mr();
    merged["state"] = json!("merged");
    merged["sha"] = json!("abc");
    fixture.payload("mr", &merged);
    fixture.payload(
        "notes",
        &json!([
            {"id": 5, "body": "coverage 80%", "created_at": "2026-11-14T08:04:00Z",
             "author": {"id": 5, "username": "project_7_bot_3f9a"}, "system": false},
            {"id": 4, "body": "try this", "created_at": "2026-11-14T08:03:00Z",
             "author": {"id": 4, "username": "visitor"}, "system": false},
            {"id": 3, "body": "changed the description", "created_at": "2026-11-14T08:02:00Z",
             "author": {"id": 1, "username": "owner"}, "system": true},
            {"id": 2, "body": "Done", "created_at": "2026-11-14T08:01:00Z",
             "author": {"id": 1, "username": "owner"}, "system": false},
            {"id": 1, "body": "please rename", "created_at": "2026-11-14T08:00:00Z",
             "type": "DiffNote", "position": {"new_path": "src/a.rs", "new_line": 7},
             "author": {"id": 2, "username": "alice"}, "system": false}
        ]),
    );
    fixture.payload(
        "members",
        &json!([
            {"id": 1, "username": "owner", "access_level": 50},
            {"id": 2, "username": "alice", "access_level": 30},
            {"id": 4, "username": "visitor", "access_level": 20},
            {"id": 5, "username": "project_7_bot_3f9a", "access_level": 40}
        ]),
    );

    let snapshot = fixture.forge.pull_request(fixture.cwd(), 42).unwrap();

    assert_eq!(snapshot.state, domain::forge::PullState::Merged);
    assert_eq!(snapshot.head.as_deref(), Some("abc"));
    assert_eq!(snapshot.checks[0].name, "test");
    assert_eq!(
        snapshot.checks[0].status,
        domain::forge::CheckStatus::Success
    );
    let notes: Vec<_> = snapshot
        .items
        .iter()
        .map(|item| (item.id.as_str(), item.own, item.access))
        .collect();
    assert_eq!(
        notes,
        [
            ("5", false, AuthorAccess::Bot),
            ("4", false, AuthorAccess::Other),
            ("2", true, AuthorAccess::Writer),
            ("1", false, AuthorAccess::Writer),
        ]
    );
    let inline = &snapshot.items[3];
    assert_eq!(
        (inline.kind, inline.path.as_deref(), inline.line),
        (
            domain::forge::PullItemKind::ReviewComment,
            Some("src/a.rs"),
            Some(7)
        )
    );
    let calls = fixture.calls();
    assert_eq!(calls.matches("view").count(), 1, "one mr view: {calls}");
    assert!(
        calls.contains("notes?sort=desc&order_by=created_at&per_page=100"),
        "{calls}"
    );
    assert!(calls.contains("user_ids[]=2"), "{calls}");
    assert!(!calls.contains("discussions"), "{calls}");
}

#[test]
fn a_failed_pipeline_or_user_read_fails_the_whole_merge_request_read() {
    let fixture = Fixture::new();
    fixture.error("ci-get", "502 Bad Gateway");

    assert!(fixture.forge.pull_request(fixture.cwd(), 42).is_err());

    let fixture = Fixture::new();
    fixture.error("api-user", "connection reset");

    assert!(fixture.forge.pull_request(fixture.cwd(), 42).is_err());
}

#[test]
fn a_locked_merge_request_is_still_open() {
    let fixture = Fixture::new();
    let mut locked = mr();
    locked["state"] = json!("locked");
    fixture.payload("mr", &locked);

    let snapshot = fixture.forge.pull_request(fixture.cwd(), 42).unwrap();

    assert_eq!(snapshot.state, domain::forge::PullState::Open);
}

#[test]
fn a_missing_merge_request_is_not_found() {
    let fixture = Fixture::new();
    fixture.error("mr-view", "404 Not Found");

    let error = fixture.forge.pull_request(fixture.cwd(), 404).unwrap_err();

    assert!(error.is_not_found(), "{error}");
}

/// Verify 3, V1: GitLab answers a deleted project, or one the account cannot see, with
/// "404 Project Not Found", which the follow read already takes as not found.
#[test]
fn a_project_gitlab_cannot_find_is_not_found() {
    let fixture = Fixture::new();
    fixture.error("mr-view", "ERROR: 404 Project Not Found");

    let error = fixture.forge.pull_request(fixture.cwd(), 7).unwrap_err();

    assert!(error.is_not_found(), "{error}");
}
