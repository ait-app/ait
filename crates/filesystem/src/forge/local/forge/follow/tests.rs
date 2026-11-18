use std::fs;
use std::path::Path;
use std::process::Command;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use domain::forge::{AuthorAccess, CHECK_LIMIT, CheckStatus, PullItemKind, PullState, ReviewState};
use serde_json::json;

use super::{FOLLOW_QUERY, parse_github, writes};
use crate::forge::local::forge::LocalForge;
use crate::forge::ports::ForgeRuntime;

fn response(pull: &serde_json::Value) -> serde_json::Value {
    json!({"data":{"viewer":{"login":"owner"},"repository":{"pullRequest":pull}}})
}

fn pull() -> serde_json::Value {
    json!({
        "number": 12, "url": "https://github.com/acme/app/pull/12", "title": "Fix login",
        "state": "OPEN", "mergedAt": null, "headRefOid": "abc",
        "commits": {"nodes": [{"commit": {"statusCheckRollup": {"contexts": {"nodes": [
            {"__typename": "CheckRun", "name": "tests", "status": "COMPLETED",
             "conclusion": "FAILURE", "detailsUrl": "https://ci/tests", "databaseId": 9},
            {"__typename": "StatusContext", "context": "lint", "state": "PENDING",
             "targetUrl": null}
        ]}}}}]},
        "reviews": {"nodes": [
            {"id": "R1", "state": "CHANGES_REQUESTED", "body": "", "url": "https://r1",
             "submittedAt": "2026-11-14T08:00:03Z", "viewerDidAuthor": false,
             "author": {"login": "alice"}},
            {"id": "R2", "state": "PENDING", "body": "draft", "url": "https://r2",
             "submittedAt": null, "viewerDidAuthor": true, "author": {"login": "owner"}}
        ]},
        "comments": {"nodes": [
            {"id": "C1", "body": "Fixed in abc", "url": "https://c1",
             "createdAt": "2026-11-14T08:00:01Z", "viewerDidAuthor": true,
             "author": {"login": "owner"}}
        ]},
        "reviewThreads": {"nodes": [
            {"path": "src/a.rs", "line": 42, "comments": {"nodes": [
                {"id": "C2", "body": "rename", "url": "https://c2",
                 "createdAt": "2026-11-14T08:00:02Z", "publishedAt": "2026-11-14T08:00:02Z",
                 "viewerDidAuthor": false, "author": {"login": "bob"},
                 "pullRequestReview": {"state": "COMMENTED"}}
            ]}}
        ]}
    })
}

#[test]
fn github_follow_read_projects_state_checks_and_marked_entries() {
    let snapshot = parse_github(&response(&pull()), 12).unwrap();

    assert_eq!(snapshot.number, 12);
    assert_eq!(snapshot.state, PullState::Open);
    assert_eq!(snapshot.head.as_deref(), Some("abc"));
    let statuses: Vec<_> = snapshot
        .checks
        .iter()
        .map(|check| (check.name.as_str(), check.status, check.run.as_deref()))
        .collect();
    assert_eq!(
        statuses,
        [
            ("lint", CheckStatus::Pending, None),
            ("tests", CheckStatus::Failure, Some("9"))
        ]
    );
    assert_eq!(
        snapshot.items.len(),
        3,
        "a pending review has no date and is left out"
    );
    let review = &snapshot.items[0];
    assert_eq!(
        (review.id.as_str(), review.kind, review.own),
        (
            "R1",
            PullItemKind::Review(ReviewState::ChangesRequested),
            false
        )
    );
    let own = &snapshot.items[1];
    assert_eq!((own.kind, own.own), (PullItemKind::Comment, true));
    let inline = &snapshot.items[2];
    assert_eq!(
        (inline.kind, inline.path.as_deref(), inline.line),
        (PullItemKind::ReviewComment, Some("src/a.rs"), Some(42))
    );
}

#[test]
fn merged_and_closed_states_and_missing_pull_requests() {
    let mut merged = pull();
    merged["state"] = json!("MERGED");
    let mut closed = pull();
    closed["state"] = json!("CLOSED");

    assert_eq!(
        parse_github(&response(&merged), 12).unwrap().state,
        PullState::Merged
    );
    assert_eq!(
        parse_github(&response(&closed), 12).unwrap().state,
        PullState::Closed
    );
    let missing = parse_github(&response(&serde_json::Value::Null), 99).unwrap_err();
    assert!(missing.is_not_found());
    assert!(!missing.is_unavailable());
}

fn repository(path: &Path) {
    fs::create_dir_all(path).unwrap();
    for arguments in [
        &["init", "-b", "main"][..],
        &["remote", "add", "origin", "git@github.com:acme/app.git"],
    ] {
        assert!(
            Command::new("git")
                .args(arguments)
                .current_dir(path)
                .status()
                .unwrap()
                .success()
        );
    }
}

#[cfg(unix)]
#[test]
fn github_cli_reads_the_origin_repository_by_number() {
    let root = tempfile::tempdir().unwrap();
    let checkout = root.path().join("repo");
    repository(&checkout);
    let body = root.path().join("body.json");
    fs::write(&body, response(&pull()).to_string()).unwrap();
    let log = root.path().join("gh.log");
    let executable = root.path().join("gh");
    fs::write(
        &executable,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{}'\ncat '{}'\n",
            log.display(),
            body.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    let forge = LocalForge::with_executable(executable);

    let snapshot = forge.pull_request(checkout.to_str().unwrap(), 12).unwrap();

    assert_eq!(snapshot.title, "Fix login");
    let calls = fs::read_to_string(log).unwrap();
    for argument in ["owner=acme", "name=app", "number=12"] {
        assert!(calls.lines().any(|line| line == argument), "{calls}");
    }
    assert!(calls.contains("viewerDidAuthor"), "{calls}");
}

#[test]
fn a_missing_cli_makes_the_read_unavailable() {
    let root = tempfile::tempdir().unwrap();
    let checkout = root.path().join("repo");
    repository(&checkout);
    let forge = LocalForge::with_executable(root.path().join("absent-gh"));

    let error = forge
        .pull_request(checkout.to_str().unwrap(), 12)
        .unwrap_err();

    assert!(error.is_unavailable(), "{error}");
}

#[test]
fn github_entries_carry_author_access_and_bots_and_publication_dates() {
    let mut read = pull();
    read["comments"]["nodes"] = json!([
        {"id": "C1", "body": "please", "url": "https://c1", "createdAt": "2026-11-14T08:00:01Z",
         "viewerDidAuthor": false, "authorAssociation": "COLLABORATOR",
         "author": {"__typename": "User", "login": "alice"}},
        {"id": "C3", "body": "run this", "url": "https://c3",
         "createdAt": "2026-11-14T08:00:04Z", "viewerDidAuthor": false,
         "authorAssociation": "NONE", "author": {"__typename": "User", "login": "stranger"}},
        {"id": "C4", "body": "coverage 80%", "url": "https://c4",
         "createdAt": "2026-11-14T08:00:05Z", "viewerDidAuthor": false,
         "authorAssociation": "MEMBER", "author": {"__typename": "Bot", "login": "codecov"}}
    ]);
    read["reviewThreads"]["nodes"][0]["comments"]["nodes"][0]["publishedAt"] =
        json!("2026-11-14T09:00:00Z");
    read["reviewThreads"]["nodes"][0]["comments"]["nodes"][0]["authorAssociation"] = json!("OWNER");

    let snapshot = parse_github(&response(&read), 12).unwrap();

    let access: Vec<_> = snapshot
        .items
        .iter()
        .map(|item| (item.id.as_str(), item.access))
        .collect();
    assert_eq!(
        access,
        [
            ("R1", AuthorAccess::Other),
            ("C1", AuthorAccess::Unconfirmed),
            ("C3", AuthorAccess::Other),
            ("C4", AuthorAccess::Bot),
            ("C2", AuthorAccess::Writer),
        ]
    );
    let inline = snapshot.items.iter().find(|item| item.id == "C2").unwrap();
    assert_eq!(
        inline.created_at.to_rfc3339(),
        "2026-11-14T09:00:00+00:00",
        "a review comment counts from when its review was published"
    );
}

#[test]
fn a_graphql_error_without_data_is_a_failure_not_a_missing_pull_request() {
    let limited = json!({"errors": [{"type": "RATE_LIMITED", "message": "API rate limit"}]});
    let missing = json!({"data": {"repository": {"pullRequest": null}},
        "errors": [{"type": "NOT_FOUND", "message": "Could not resolve"}]});

    let error = parse_github(&limited, 12).unwrap_err();

    assert!(!error.is_not_found(), "{error}");
    assert!(parse_github(&missing, 12).unwrap_err().is_not_found());
}

#[test]
fn the_follow_query_reads_as_many_checks_as_the_domain_keeps() {
    assert!(FOLLOW_QUERY.contains(&format!("contexts(first: {CHECK_LIMIT})")));
    for field in ["authorAssociation", "__typename login", "publishedAt"] {
        assert!(FOLLOW_QUERY.contains(field), "{field}");
    }
}

#[test]
fn comments_of_an_unsubmitted_review_are_left_out() {
    let mut read = pull();
    read["reviewThreads"]["nodes"][0]["comments"]["nodes"] = json!([
        {"id": "P1", "body": "half-written", "url": "https://p1",
         "createdAt": "2026-11-14T08:00:02Z", "publishedAt": null, "viewerDidAuthor": true,
         "author": {"login": "owner"}, "pullRequestReview": {"state": "PENDING"}},
        {"id": "P2", "body": "drafted", "url": "https://p2",
         "createdAt": "2026-11-14T08:00:02Z", "publishedAt": "2026-11-14T08:00:03Z",
         "viewerDidAuthor": true, "author": {"login": "owner"},
         "pullRequestReview": {"state": "PENDING"}},
        {"id": "P3", "body": "no date", "url": "https://p3",
         "createdAt": "2026-11-14T08:00:02Z", "publishedAt": null, "viewerDidAuthor": false,
         "author": {"login": "bob"}, "pullRequestReview": {"state": "COMMENTED"}},
        {"id": "P4", "body": "submitted", "url": "https://p4",
         "createdAt": "2026-11-14T08:00:02Z", "publishedAt": "2026-11-14T08:00:05Z",
         "viewerDidAuthor": false, "author": {"login": "bob"},
         "pullRequestReview": {"state": "COMMENTED"}}
    ]);

    let snapshot = parse_github(&response(&read), 12).unwrap();

    let inline: Vec<_> = snapshot
        .items
        .iter()
        .filter(|item| item.kind == PullItemKind::ReviewComment)
        .map(|item| item.id.as_str())
        .collect();
    assert_eq!(inline, ["P4"]);
    assert!(FOLLOW_QUERY.contains("pullRequestReview { state }"));
}

#[test]
fn only_admin_maintain_and_write_roles_can_write() {
    for (response, writer) in [
        (json!({"permission": "admin", "role_name": "admin"}), true),
        (
            json!({"permission": "write", "role_name": "maintain"}),
            true,
        ),
        (json!({"permission": "write", "role_name": "write"}), true),
        (json!({"permission": "read", "role_name": "triage"}), false),
        (json!({"permission": "read", "role_name": "read"}), false),
        (json!({"permission": "none"}), false),
        (json!({}), false),
    ] {
        assert_eq!(writes(&response), writer, "{response}");
    }
}

#[cfg(unix)]
fn scripted_gh(root: &Path, script: &str) -> LocalForge {
    let executable = root.join("gh");
    fs::write(&executable, format!("#!/bin/sh\n{script}\n")).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    LocalForge::with_executable(executable)
}

#[cfg(unix)]
#[test]
fn write_access_is_read_from_the_collaborator_permission() {
    let root = tempfile::tempdir().unwrap();
    let checkout = root.path().join("repo");
    repository(&checkout);
    let log = root.path().join("gh.log");
    let forge = scripted_gh(
        root.path(),
        &format!(
            "printf '%s\\n' \"$@\" >> '{}'\necho '{{\"permission\":\"write\",\"role_name\":\"maintain\"}}'",
            log.display()
        ),
    );
    let cwd = checkout.to_str().unwrap();

    assert_eq!(forge.can_write(cwd, "maria"), Ok(true));
    assert_eq!(forge.can_write(cwd, "../../user"), Ok(false), "not a login");

    let calls = fs::read_to_string(log).unwrap();
    assert!(
        calls
            .lines()
            .any(|line| line == "repos/acme/app/collaborators/maria/permission"),
        "{calls}"
    );
    assert!(!calls.contains("../"), "{calls}");
}

#[cfg(unix)]
#[test]
fn an_unknown_login_cannot_write_and_other_failures_fail_the_lookup() {
    let root = tempfile::tempdir().unwrap();
    let checkout = root.path().join("repo");
    repository(&checkout);
    let cwd = checkout.to_str().unwrap();
    let unknown = scripted_gh(root.path(), "echo 'gh: Not Found (HTTP 404)' >&2\nexit 1");

    assert_eq!(unknown.can_write(cwd, "ghost"), Ok(false));

    let limited = scripted_gh(
        root.path(),
        "echo 'gh: API rate limit exceeded (HTTP 403)' >&2\nexit 1",
    );
    assert!(limited.can_write(cwd, "maria").is_err());
}

/// Re-review 2, R1: GitHub answers HTTP 403 "Must have push access to view collaborator
/// permission" when the host's account cannot push, such as when it pushed the branch to a
/// fork. That never changes on a later read, so the lookup answers that Ait cannot confirm the
/// author can write, while a rate limit (also HTTP 403) stays a failure to try again.
#[cfg(unix)]
#[test]
fn a_lookup_github_refuses_without_push_access_cannot_confirm_the_author() {
    let root = tempfile::tempdir().unwrap();
    let checkout = root.path().join("repo");
    repository(&checkout);
    let cwd = checkout.to_str().unwrap();
    let refused = scripted_gh(
        root.path(),
        "echo 'gh: Must have push access to view collaborator permission. (HTTP 403)' >&2\nexit 1",
    );

    assert_eq!(refused.can_write(cwd, "maria"), Ok(false));

    let limited = scripted_gh(
        root.path(),
        "echo 'gh: API rate limit exceeded for user ID 1. (HTTP 403)' >&2\nexit 1",
    );
    assert!(limited.can_write(cwd, "maria").is_err());
}

/// Verify 3, V1: a deleted repository, or one the host's account can no longer see, made
/// `gh api graphql` exit 1 with "Could not resolve to a Repository", which was not
/// `NotFound`, so the ticket never stopped following it. The output is what `gh` 2.x prints.
#[cfg(unix)]
#[test]
fn a_repository_github_cannot_resolve_reads_as_not_found() {
    let root = tempfile::tempdir().unwrap();
    let checkout = root.path().join("repo");
    repository(&checkout);
    let cwd = checkout.to_str().unwrap();
    let gone = scripted_gh(
        root.path(),
        "echo '{\"data\":{\"repository\":null},\"errors\":[{\"type\":\"NOT_FOUND\",\"path\":[\"repository\"],\"message\":\"Could not resolve to a Repository with the name '\\''acme/app'\\''.\"}]}'\necho \"gh: Could not resolve to a Repository with the name 'acme/app'.\" >&2\nexit 1",
    );

    assert!(gone.pull_request(cwd, 12).unwrap_err().is_not_found());

    let pull_gone = scripted_gh(
        root.path(),
        "echo \"gh: Could not resolve to a PullRequest with the number of 12.\" >&2\nexit 1",
    );
    assert!(pull_gone.pull_request(cwd, 12).unwrap_err().is_not_found());
    let failing = scripted_gh(root.path(), "echo 'gh: HTTP 502' >&2\nexit 1");
    assert!(!failing.pull_request(cwd, 12).unwrap_err().is_not_found());
}
