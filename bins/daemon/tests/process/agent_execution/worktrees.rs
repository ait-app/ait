//! Agent creation options exercised through the real Git and native-session adapters.

use std::path::PathBuf;
use std::time::Duration;

use super::super::worktrees::{branch, create_repository, run};
use super::*;

#[path = "worktrees/pr_checkout.rs"]
mod pr_checkout;

#[tokio::test]
async fn agent_worktree_creation_is_idempotent_and_auto_archive_removes_its_checkout() {
    let super::super::native::NativeFixture { root, cwd, path } =
        super::super::native::NativeFixture::new();
    create_repository(&cwd);
    std::fs::write(
        cwd.join("packages/app/ait.json"),
        r#"{"worktree":{"setup":["printf x >> setup-count.txt"]}}"#,
    )
    .unwrap();
    let state = root.path().join("state");
    let log = root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&path));
    let address = ready(&mut process, &log).await;
    let mut methods = METHODS.to_vec();
    methods.extend(["workspace.worktree.list.request", "workspace.list.request"]);
    let mut client = connect(&address, &methods).await;
    let params = json!({"idempotencyKey":"new-worktree", "autoArchive":true,
        "worktree":{"mode":"branch-off","newBranch":"Feature Review","base":"main"},
        "config":{"provider":"codex","cwd":cwd.join("packages/app")}});
    let created = request(&mut client, "agent.create.request", params.clone()).await;
    assert_eq!(created["type"], "response", "{created}");
    let repeated = request(&mut client, "agent.create.request", params).await;
    assert_eq!(created["result"]["agentId"], repeated["result"]["agentId"]);
    let id = &created["result"]["agentId"];
    let directory = PathBuf::from(created["result"]["agent"]["cwd"].as_str().unwrap());
    assert!(directory.ends_with("packages/app"));
    assert_ne!(directory, cwd.join("packages/app"));
    assert_eq!(branch(&directory), "feature-review");
    assert_setup_once(&directory).await;
    let sibling_id = start_workspace_sibling(&mut client, &created).await;
    let listed = request(
        &mut client,
        "workspace.worktree.list.request",
        json!({"cwd":cwd}),
    )
    .await;
    assert_eq!(
        listed["result"]["worktrees"].as_array().unwrap().len(),
        1,
        "{listed}"
    );
    let sent = request(
        &mut client,
        "agent.message.send.request",
        json!({"agentId":id,"text":"finish worktree"}),
    )
    .await;
    assert_eq!(sent["result"]["accepted"], true, "{sent}");
    let removed = tokio::time::timeout(Duration::from_secs(10), async {
        while directory.exists() {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await;
    assert!(
        removed.is_ok(),
        "agent: {}; workspaces: {}; log: {}",
        request(&mut client, "agent.get.request", json!({"agentId":id})).await,
        std::fs::read_to_string(state.join("projects/workspaces.json")).unwrap(),
        std::fs::read_to_string(&log).unwrap()
    );
    let archived = request(&mut client, "agent.get.request", json!({"agentId":id})).await;
    assert!(
        archived["result"]["agent"]["archivedAt"].is_string(),
        "{archived}"
    );
    let sibling = request(
        &mut client,
        "agent.get.request",
        json!({"agentId":sibling_id}),
    )
    .await;
    assert!(
        sibling["result"]["agent"]["archivedAt"].is_string(),
        "{sibling}"
    );
    assert!(cwd.join("packages/app/tracked.txt").exists());
    assert_eq!(branch(&cwd), "main");
    let listed = request(
        &mut client,
        "workspace.worktree.list.request",
        json!({"cwd":cwd}),
    )
    .await;
    assert_eq!(listed["result"]["worktrees"], json!([]));
    let workspaces = request(&mut client, "workspace.list.request", json!({})).await;
    assert_eq!(
        workspaces["result"]["entries"].as_array().unwrap().len(),
        0,
        "{workspaces}"
    );
    let persisted: Value =
        serde_json::from_slice(&std::fs::read(state.join("projects/workspaces.json")).unwrap())
            .unwrap();
    assert!(persisted[0]["archivedAt"].is_string());
    client.close(None).await.unwrap();
    terminate(&mut process).await;
}

#[tokio::test]
async fn agent_worktree_checkout_legacy_name_and_failed_native_creation_preserve_source() {
    let super::super::native::NativeFixture { root, cwd, path } =
        super::super::native::NativeFixture::new();
    create_repository(&cwd);
    run(&cwd, &["branch", "existing"]);
    let state = root.path().join("state");
    let log = root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&path));
    let address = ready(&mut process, &log).await;
    let mut methods = METHODS.to_vec();
    methods.push("workspace.worktree.list.request");
    let mut client = connect(&address, &methods).await;
    for (extra, expected) in [
        (
            json!({"worktree":{"mode":"checkout-branch","branch":"existing"}}),
            "existing",
        ),
        (json!({"worktreeName":"Legacy Branch"}), "legacy-branch"),
    ] {
        let mut params = json!({"config":{"provider":"codex","cwd":cwd}});
        params
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let created = request(&mut client, "agent.create.request", params).await;
        assert_eq!(created["type"], "response", "{created}");
        let directory = PathBuf::from(created["result"]["agent"]["cwd"].as_str().unwrap());
        assert_eq!(branch(&directory), expected);
    }
    std::fs::write(cwd.join("behavior"), "error").unwrap();
    run(&cwd, &["add", "behavior"]);
    run(
        &cwd,
        &[
            "-c",
            "user.name=Server Test",
            "-c",
            "user.email=server@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "native failure fixture",
        ],
    );
    let failed = request(
        &mut client,
        "agent.create.request",
        json!({
            "worktree":{"mode":"branch-off","newBranch":"failed-creation"},
            "config":{"provider":"codex","cwd":cwd,"model":"missing-model"}
        }),
    )
    .await;
    assert_eq!(failed["type"], "error", "{failed}");
    let listed = request(
        &mut client,
        "workspace.worktree.list.request",
        json!({"cwd":cwd}),
    )
    .await;
    assert_eq!(
        listed["result"]["worktrees"].as_array().unwrap().len(),
        2,
        "{listed}"
    );
    assert_eq!(branch(&cwd), "main");
    client.close(None).await.unwrap();
    terminate(&mut process).await;
}

async fn start_workspace_sibling(
    client: &mut super::super::transport::Socket,
    created: &Value,
) -> Value {
    // Independent Agents in the owned Workspace must also stop before checkout removal.
    let sibling = request(
        client,
        "agent.create.request",
        json!({
            "workspaceId":created["result"]["agent"]["workspaceId"],
            "config":{"provider":"codex","cwd":created["result"]["agent"]["cwd"]}
        }),
    )
    .await;
    assert_eq!(sibling["type"], "response", "{sibling}");
    let sibling_id = sibling["result"]["agentId"].clone();
    let active = request(
        client,
        "agent.message.send.request",
        json!({"agentId":sibling_id,"text":"hang"}),
    )
    .await;
    assert_eq!(active["result"]["accepted"], true, "{active}");
    sibling_id
}

#[tokio::test]
async fn agent_legacy_git_options_change_only_a_clean_source_checkout_and_replay_once() {
    let super::super::native::NativeFixture { root, cwd, path } =
        super::super::native::NativeFixture::new();
    create_repository(&cwd);
    let state = root.path().join("state");
    let log = root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&path));
    let address = ready(&mut process, &log).await;
    let mut client = connect(&address, METHODS).await;
    let params = json!({"idempotencyKey":"legacy-directory", "git":{"createNewBranch":true,"newBranchName":"Feature Legacy","baseBranch":"main"},
        "config":{"provider":"codex","cwd":cwd}});
    let created = request(&mut client, "agent.create.request", params.clone()).await;
    assert_eq!(created["type"], "response", "{created}");
    assert_eq!(branch(&cwd), "feature-legacy");
    let repeated = request(&mut client, "agent.create.request", params).await;
    assert_eq!(created["result"]["agentId"], repeated["result"]["agentId"]);
    let dirty = request(
        &mut client,
        "agent.create.request",
        json!({"git":{"baseBranch":"main"},"config":{"provider":"codex","cwd":cwd}}),
    )
    .await;
    assert_eq!(dirty["type"], "error", "{dirty}");
    assert_eq!(branch(&cwd), "feature-legacy");
    request(
        &mut client,
        "agent.delete.request",
        json!({"agentId":created["result"]["agentId"]}),
    )
    .await;
    run(&cwd, &["clean", "-fd"]);
    let switched = request(
        &mut client,
        "agent.create.request",
        json!({"git":{"baseBranch":"main"},"config":{"provider":"codex","cwd":cwd}}),
    )
    .await;
    assert_eq!(switched["type"], "response", "{switched}");
    assert_eq!(branch(&cwd), "main");
    client.close(None).await.unwrap();
    terminate(&mut process).await;
}

async fn assert_setup_once(directory: &std::path::Path) {
    let marker = directory.join("setup-count.txt");
    tokio::time::timeout(Duration::from_secs(10), async {
        // Shell redirection creates the marker before printf writes its content.
        while !marker.exists() || std::fs::read_to_string(&marker).unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(std::fs::read_to_string(marker).unwrap(), "x");
}
