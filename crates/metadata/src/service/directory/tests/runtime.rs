//! Checkout presentation facts survive listing, mutations, and directory subscriptions.

use std::sync::Arc;

use model::ServerMessage;
use model::workspace::runtime::{
    WorkspaceCheckSnapshot, WorkspaceDiffStat, WorkspaceForgeSnapshot, WorkspaceGitSnapshot,
    WorkspacePullRequestSnapshot, WorkspaceRuntimeSnapshot, WorkspaceRuntimeSource,
};
use serde_json::json;

use super::*;
use crate::rpc::directory::{execute, listing};

#[derive(Debug, Default)]
struct Runtime {
    snapshot: Mutex<WorkspaceRuntimeSnapshot>,
    reads: Mutex<Vec<String>>,
}

impl WorkspaceRuntimeSource for Runtime {
    fn snapshot(&self, cwd: &str) -> WorkspaceRuntimeSnapshot {
        self.reads.lock().unwrap().push(cwd.to_owned());
        self.snapshot.lock().unwrap().clone()
    }
}

fn populated() -> WorkspaceRuntimeSnapshot {
    WorkspaceRuntimeSnapshot {
        git: Some(WorkspaceGitSnapshot {
            current_branch: Some("feature".to_owned()),
            remote_url: Some("https://gitlab.com/acme/repo.git".to_owned()),
            is_managed_worktree: true,
            is_dirty: Some(true),
            ahead_behind: Some((3, 1)),
            ahead_of_origin: Some(2),
            behind_of_origin: Some(0),
            diff_stat: Some(WorkspaceDiffStat {
                additions: 2500,
                deletions: 35,
            }),
        }),
        forge: Some(WorkspaceForgeSnapshot {
            features_enabled: true,
            forge: Some("gitlab".to_owned()),
            pull_request: Some(WorkspacePullRequestSnapshot {
                number: Some(136),
                url: "https://gitlab.com/acme/repo/-/merge_requests/136".to_owned(),
                title: "Workspace badges".to_owned(),
                state: "merged".to_owned(),
                base_ref_name: "main".to_owned(),
                head_ref_name: "feature".to_owned(),
                is_merged: true,
                is_draft: false,
                mergeable: "MERGEABLE".to_owned(),
                checks: Some("success")
                    .into_iter()
                    .map(|status| WorkspaceCheckSnapshot {
                        name: "test".to_owned(),
                        status: status.to_owned(),
                        url: None,
                        workflow: Some("CI".to_owned()),
                        duration: None,
                        traits: None,
                    })
                    .collect(),
                checks_status: "success".to_owned(),
                review_decision: Some("approved".to_owned()),
                repo_owner: Some("acme".to_owned()),
                repo_name: Some("repo".to_owned()),
                github: None,
            }),
            error: None,
        }),
    }
}

#[test]
fn runtime_projection_preserves_failed_checks_and_unknown_forge_states() {
    let source = Arc::new(Runtime::default());
    let mut directory = directory().with_runtime_source(source.clone());
    for (mergeable, check, review) in [
        ("CONFLICTING", "failure", "changes_requested"),
        ("UNKNOWN", "pending", "review_required"),
        ("UNKNOWN", "skipped", "unknown"),
        ("UNKNOWN", "cancelled", "approved"),
    ] {
        let mut snapshot = populated();
        let forge = snapshot.forge.as_mut().unwrap();
        forge.error = Some("refresh failed".to_owned());
        let pr = forge.pull_request.as_mut().unwrap();
        pr.mergeable = mergeable.to_owned();
        pr.checks[0].status = check.to_owned();
        pr.checks_status = check.to_owned();
        pr.review_decision = Some(review.to_owned());
        *source.snapshot.lock().unwrap() = snapshot;
        let list = execute(&mut directory, "workspace.list.request", json!({})).unwrap();
        let runtime = &list["entries"][0]["githubRuntime"];
        assert_eq!(runtime["error"]["message"], "refresh failed");
        assert_eq!(runtime["pullRequest"]["checks"][0]["status"], check);
        assert_eq!(runtime["pullRequest"]["number"], 136);
        assert_eq!(runtime["pullRequest"]["mergeable"], mergeable);
        assert_eq!(
            runtime["pullRequest"]["checksStatus"],
            match check {
                "failure" => "failure",
                "pending" => "pending",
                _ => "none",
            }
        );
        assert_eq!(
            runtime["pullRequest"]["reviewDecision"],
            match review {
                "changes_requested" => "changes_requested",
                "approved" => "approved",
                _ => "pending",
            }
        );
    }
}

#[test]
fn workspace_runtime_projects_sidebar_and_hover_card_facts_once_per_directory() {
    let source = Arc::new(Runtime::default());
    *source.snapshot.lock().unwrap() = populated();
    let mut directory = directory().with_runtime_source(source.clone());
    let mut second = workspace();
    second.workspace_id = "wks_b".to_owned();
    directory
        .workspaces
        .upsert(&second, WorkspaceMutationContext::default())
        .unwrap();

    let list = execute(&mut directory, "workspace.list.request", json!({})).unwrap();
    assert_eq!(source.reads.lock().unwrap().as_slice(), ["/tmp/alpha"]);
    for entry in list["entries"].as_array().unwrap() {
        assert_eq!(entry["diffStat"], json!({"additions":2500,"deletions":35}));
        assert_eq!(entry["gitRuntime"]["currentBranch"], "feature");
        assert_eq!(
            entry["gitRuntime"]["aheadBehind"],
            json!({"ahead":3,"behind":1})
        );
        assert_eq!(entry["gitRuntime"]["isDirty"], true);
        assert_eq!(entry["forge"], "gitlab");
        assert_eq!(entry["githubRuntime"]["pullRequest"]["number"], 136);
        assert_eq!(entry["githubRuntime"]["pullRequest"]["isMerged"], true);
        assert_eq!(
            entry["githubRuntime"]["pullRequest"]["checksStatus"],
            "success"
        );
        assert_eq!(
            entry["githubRuntime"]["pullRequest"]["checks"][0]["status"],
            "success"
        );
    }
    let opened = execute(
        &mut directory,
        "workspace.open.request",
        json!({"cwd":"/tmp/alpha"}),
    )
    .unwrap();
    assert_eq!(
        opened["workspace"]["diffStat"],
        list["entries"][0]["diffStat"]
    );
}

#[test]
fn workspace_runtime_updates_use_existing_sync_sequences_and_explicit_nulls() {
    let source = Arc::new(Runtime::default());
    let directory = directory().with_runtime_source(source.clone());
    let request = serde_json::from_value(json!({"subscribe":{},"sync":{}})).unwrap();
    let (first, mut observer) =
        listing::prepare(&directory, request, "runtime-sub".to_owned()).unwrap();
    assert!(observer.update(&directory.clone()).unwrap().is_empty());

    *source.snapshot.lock().unwrap() = populated();
    let events = observer.update(&directory.clone()).unwrap();
    let [ServerMessage::Event { method, params }] = events.as_slice() else {
        panic!("Workspace update expected");
    };
    assert_eq!(method, "workspace.update");
    assert_eq!(params["subscriptionId"], "runtime-sub");
    assert_eq!(params["generation"], first["sync"]["generation"]);
    assert_eq!(params["seq"], 2);
    assert_eq!(
        params["workspace"]["githubRuntime"]["pullRequest"]["state"],
        "merged"
    );
    assert!(observer.update(&directory).unwrap().is_empty());

    *source.snapshot.lock().unwrap() = WorkspaceRuntimeSnapshot::default();
    let cleared = observer.update(&directory).unwrap();
    let [ServerMessage::Event { params, .. }] = cleared.as_slice() else {
        panic!("Clearing update expected");
    };
    assert_eq!(params["seq"], 3);
    for field in ["diffStat", "gitRuntime", "githubRuntime"] {
        assert_eq!(
            params["workspace"].get(field),
            Some(&serde_json::Value::Null)
        );
    }
}

#[test]
fn synced_workspace_subscription_emits_one_removal_with_a_monotonic_sequence() {
    let directory = directory();
    let request = serde_json::from_value(json!({"subscribe":{},"sync":{}})).unwrap();
    let (initial, mut observer) =
        listing::prepare(&directory, request, "remove-sub".to_owned()).unwrap();
    let id = initial["entries"][0]["id"].as_str().unwrap();
    directory.workspaces.remove(id).unwrap();
    let updates = observer.update(&directory).unwrap();
    let [ServerMessage::Event { method, params }] = updates.as_slice() else {
        panic!("one removal expected: {updates:?}");
    };
    assert_eq!(method, "workspace.update");
    assert_eq!(params["kind"], "remove");
    assert_eq!(params["id"], id);
    assert_eq!(params["generation"], initial["sync"]["generation"]);
    assert!(params["seq"].as_u64().unwrap() > initial["sync"]["headSeq"].as_u64().unwrap());
    assert!(observer.update(&directory).unwrap().is_empty());
}

#[test]
fn workspace_runtime_keeps_local_facts_when_forge_is_unavailable() {
    let source = Arc::new(Runtime::default());
    let mut snapshot = populated();
    snapshot.forge = Some(WorkspaceForgeSnapshot {
        error: Some("CLI unavailable".to_owned()),
        ..Default::default()
    });
    *source.snapshot.lock().unwrap() = snapshot;
    let mut directory = directory().with_runtime_source(source);
    let response = execute(&mut directory, "workspace.list.request", json!({})).unwrap();
    let entry = &response["entries"][0];
    assert_eq!(entry["diffStat"]["additions"], 2500);
    assert_eq!(entry["githubRuntime"]["featuresEnabled"], false);
    assert_eq!(
        entry["githubRuntime"]["pullRequest"],
        serde_json::Value::Null
    );
    assert_eq!(
        entry["githubRuntime"]["error"]["message"],
        "CLI unavailable"
    );
}
