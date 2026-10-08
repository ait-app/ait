//! Real server + WebSocket + Git, with an isolated offline glab process.

use super::super::transport::method_names;

use super::*;
use crate::unix::TOKEN;

#[tokio::test]
async fn server_routes_self_managed_gitlab_operations_without_github_queries() {
    let root = tempfile::tempdir().unwrap();
    let repository = root.path().join("repository");
    let remote = root.path().join("remote.git");
    create_repository(&repository, &remote);
    fs::write(
        repository.join("ait.json"),
        r#"{"worktree":{"setup":["touch setup-ran"]}}"#,
    )
    .unwrap();
    run(&repository, &["add", "ait.json"]);
    run(&repository, &["commit", "-m", "fork setup"]);
    let url = "https://code.company.test/group/subgroup/project.git";
    run(&repository, &["remote", "set-url", "origin", url]);
    run(
        &repository,
        &[
            "config",
            &format!("url.{}.insteadOf", remote.display()),
            url,
        ],
    );
    let bin = root.path().join("bin");
    fs::create_dir(&bin).unwrap();
    script(
        &bin.join("gh"),
        "if [ \"$1 $2\" != 'auth status' ]; then echo wrong-forge >> \"${0%/*}/github-query\"; fi\nexit 1",
    );
    script(&bin.join("glab"), include_str!("gitlab/glab.sh"));
    let path = prepend_path(&bin);
    let state = root.path().join("state");
    let log = root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&path));
    let address = ready(&mut process, &log).await;
    let info: model::server::ServerInfo = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(format!("http://{address}/v1/server/info"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        info.features
            .iter()
            .any(|feature| feature == "forge-gitlab-v1")
    );
    let mut methods = method_names(filesystem::forge::rpc::forge::METHODS);
    methods.extend(["workspace.create.request", "workspace.setup.status.request"]);
    let mut client = connect(&address, &methods).await;
    let status = request(
        &mut client,
        "checkout.pr.status.request",
        json!({"cwd":repository}),
    )
    .await;
    assert_eq!(status["result"]["forge"], "gitlab", "{status}");
    assert_eq!(status["result"]["status"]["number"], 42);
    assert_eq!(
        status["result"]["status"]["projectPath"],
        "group/subgroup/project"
    );
    assert_eq!(
        status["result"]["status"]["forgeSpecific"]["forge"],
        "gitlab"
    );
    assert!(status["result"]["error"].is_null());
    let search = request(
        &mut client,
        "forge.search.request",
        json!({"cwd":repository,"query":"fix"}),
    )
    .await;
    assert_eq!(search["result"]["authState"], "authenticated", "{search}");
    assert_eq!(search["result"]["items"][0]["forge"], "gitlab");
    let created = request(
        &mut client,
        "checkout.pr.create.request",
        json!({"cwd":repository,"title":"Fix","body":"Details","baseRef":"main"}),
    )
    .await;
    assert_eq!(created["result"]["number"], 42, "{created}");
    let merged = request(
        &mut client,
        "checkout.pr.merge.request",
        json!({"cwd":repository,"mergeMethod":"squash"}),
    )
    .await;
    assert_eq!(merged["result"]["success"], true, "{merged}");
    for enabled in [true, false] {
        let mut params = json!({"cwd":repository,"enabled":enabled});
        if enabled {
            params["mergeMethod"] = json!("merge");
        }
        let result = request(&mut client, "checkout.forge.set_auto_merge.request", params).await;
        assert_eq!(result["result"]["success"], true, "{result}");
    }
    let timeline = request(
        &mut client,
        "checkout.pr.timeline.request",
        json!({"cwd":repository,"prNumber":42,"repoOwner":"group","repoName":"project"}),
    )
    .await;
    assert_eq!(
        timeline["result"]["items"][0]["body"], "Looks good",
        "{timeline}"
    );
    let checks = request(
        &mut client,
        "checkout.forge.get_check_details.request",
        json!({"cwd":repository,"checkRunId":7,"changeRequestNumber":42}),
    )
    .await;
    assert_eq!(checks["result"]["details"]["pipeline"]["id"], 7, "{checks}");
    assert!(!bin.join("github-query").exists());
    let calls = fs::read_to_string(bin.join("calls")).unwrap();
    assert!(calls.contains("--hostname code.company.test"));
    assert!(calls.contains("--repo https://code.company.test/group/subgroup/project"));
    assert!(calls.contains("--auto-merge=false"));
    assert!(calls.contains("cancel_merge_when_pipeline_succeeds"));

    // Errors must retain the GitLab brand through the RPC projection.
    fs::write(
        bin.join("fail"),
        "GraphQL: forbidden query should never be sent",
    )
    .unwrap();
    let failure = request(
        &mut client,
        "checkout.pr.status.request",
        json!({"cwd":repository}),
    )
    .await;
    assert_eq!(failure["result"]["forge"], "gitlab", "{failure}");
    assert!(
        failure["result"]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("GraphQL")
    );
    fs::remove_file(bin.join("fail")).unwrap();
    run(
        &remote,
        &[
            "update-ref",
            "refs/merge-requests/42/head",
            "refs/heads/feature",
        ],
    );
    let (checkout, _) = crate::unix::transport::creation(
        &mut client,
        "workspace.create.request",
        json!({"source":{"kind":"worktree", "cwd":repository,
            "checkoutSource":{"kind":"change_request", "forge":"gitlab", "number":42}}}),
    )
    .await;
    assert!(checkout["result"]["error"].is_null(), "{checkout}");
    let directory = checkout["result"]["workspace"]["workspaceDirectory"]
        .as_str()
        .unwrap();
    assert!(Path::new(directory).join("feature.txt").is_file());
    assert!(!Path::new(directory).join("setup-ran").exists());
    let (setup, _) = crate::unix::transport::creation(
        &mut client,
        "workspace.setup.status.request",
        json!({"workspaceId":checkout["result"]["workspace"]["id"]}),
    )
    .await;
    assert_eq!(setup["result"]["snapshot"]["status"], "blocked", "{setup}");
    assert_eq!(
        setup["result"]["snapshot"]["blockedSource"]["forge"],
        "gitlab"
    );
    assert!(!bin.join("github-query").exists());
    client.close(None).await.unwrap();
    terminate(&mut process).await;
}

fn script(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}
