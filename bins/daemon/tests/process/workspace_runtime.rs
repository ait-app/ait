//! Real server transport regression for sidebar Git, PR, and CI presentation facts.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

use super::transport::{Socket, connect, receive, request};
use super::{ready, start_with_path, terminate};

fn git(repo: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(arguments)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn prepare(root: &Path) -> std::path::PathBuf {
    let repo = root.join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    fs::write(repo.join("tracked.txt"), "base\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "base"]);
    git(&repo, &["checkout", "-b", "feature"]);
    git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/repo.git",
        ],
    );
    fs::write(repo.join("tracked.txt"), "base\nnew\n").unwrap();
    let bin = root.join("bin");
    fs::create_dir(&bin).unwrap();
    fs::write(bin.join("gh"), r#"#!/bin/sh
printf '%s' '{"number":136,"url":"https://github.com/acme/repo/pull/136","title":"Sidebar","state":"MERGED","mergedAt":"2026-10-02T00:00:00Z","isDraft":false,"baseRefName":"main","headRefName":"feature","mergeable":"MERGEABLE","reviewDecision":"APPROVED","statusCheckRollup":[{"__typename":"CheckRun","name":"test","status":"COMPLETED","conclusion":"SUCCESS"}]}'
"#).unwrap();
    fs::set_permissions(bin.join("gh"), fs::Permissions::from_mode(0o755)).unwrap();
    repo
}

async fn next_workspace(client: &mut Socket) -> Value {
    loop {
        let event = receive(client).await;
        if event["method"] == "workspace.update" && event["params"]["kind"] == "upsert" {
            return event["params"]["workspace"].clone();
        }
    }
}

#[tokio::test]
async fn binary_workspace_runtime_streams_sidebar_facts_and_live_edits() {
    let root = tempfile::tempdir().unwrap();
    let repo = prepare(root.path());
    let state = root.path().join("server");
    let log = root.path().join("server.log");
    let path = std::env::join_paths(
        std::iter::once(root.path().join("bin"))
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let mut process = start_with_path(&state, &log, Some(&path));
    let address = ready(&mut process, &log).await;
    let capabilities = [
        "workspace.open.request",
        "workspace.list.request",
        "checkout.diff.get.request",
        "subscription.release.request",
    ];
    let mut client = connect(&address, &capabilities).await;
    let opened = request(&mut client, "workspace.open.request", json!({"cwd":repo})).await;
    assert!(opened["result"]["error"].is_null(), "{opened}");
    let workspace_id = opened["result"]["workspace"]["id"].clone();
    let subscribed = request(
        &mut client,
        "workspace.list.request",
        json!({"subscribe":{},"sync":{}}),
    )
    .await;
    let subscription_id = subscribed["result"]["subscriptionId"].clone();
    assert!(subscription_id.is_string(), "{subscribed}");

    let mut workspace = subscribed["result"]["entries"][0].clone();
    while workspace["githubRuntime"]["pullRequest"]["number"] != 136 {
        workspace = next_workspace(&mut client).await;
    }
    assert_eq!(workspace["id"], workspace_id);
    assert_eq!(workspace["diffStat"], json!({"additions":1,"deletions":0}));
    assert_eq!(workspace["gitRuntime"]["currentBranch"], "feature");
    assert_eq!(workspace["gitRuntime"]["isDirty"], true);
    assert_eq!(workspace["githubRuntime"]["pullRequest"]["isMerged"], true);
    assert_eq!(
        workspace["githubRuntime"]["pullRequest"]["checksStatus"],
        "success"
    );
    assert_eq!(workspace["forge"], "github");
    serde_json::from_value::<domain::workspace::protocol::workspace::WorkspaceDescriptorPayload>(
        workspace,
    )
    .unwrap();

    fs::write(repo.join("tracked.txt"), "base\nnew\nnewer\n").unwrap();
    loop {
        let workspace = next_workspace(&mut client).await;
        if workspace["diffStat"]["additions"] == 2 {
            break;
        }
    }
    let mut reader = connect(&address, &capabilities).await;
    let list = request(&mut reader, "workspace.list.request", json!({})).await;
    assert_eq!(list["result"]["entries"][0]["diffStat"]["additions"], 2);
    assert_eq!(
        list["result"]["entries"][0]["githubRuntime"]["pullRequest"]["number"],
        136
    );

    git(&repo, &["commit", "-am", "feature"]);
    git(&repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    let diff = request(
        &mut reader,
        "checkout.diff.get.request",
        json!({"cwd":repo,"compare":{"mode":"base","baseRef":"main"}}),
    )
    .await;
    assert!(diff["result"]["error"].is_null(), "{diff}");
    assert_eq!(diff["result"]["files"], json!([]));
    loop {
        let workspace = next_workspace(&mut client).await;
        if workspace["diffStat"].is_null() {
            assert_eq!(workspace["gitRuntime"]["isDirty"], false);
            break;
        }
    }

    let released = request(
        &mut client,
        "subscription.release.request",
        json!({"subscriptionId":subscription_id}),
    )
    .await;
    assert_eq!(released["type"], "response");
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(500),
            futures_util::StreamExt::next(&mut client)
        )
        .await
        .is_err()
    );
    terminate(&mut process).await;
}
