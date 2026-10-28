use serde_json::json;

use super::transport::{connect, request};
use super::{ready, start, terminate};

const METHODS: &[&str] = &[
    "agent.skills.get_status.request",
    "agent.skills.reconcile.request",
    "agent.skills.uninstall.request",
    "agent.skills.save_selection.request",
    "agent.skills.import_legacy_selection.request",
];

#[tokio::test]
async fn skills_are_installed_confirmed_and_persisted_through_websocket() {
    Box::pin(skills_workflow(false)).await;
}

#[tokio::test]
async fn skills_are_installed_confirmed_and_persisted_through_single_connection() {
    Box::pin(skills_workflow(true)).await;
}

async fn skills_workflow(single: bool) {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("state");
    let bundle = data.join("skills-bundle/example");
    std::fs::create_dir_all(&bundle).unwrap();
    std::fs::write(bundle.join("SKILL.md"), "example skill").unwrap();
    let log = root.path().join("server.log");
    let mut process = start(&data, &log);
    let address = ready(&mut process, &log).await;
    let mut capabilities = METHODS.to_vec();
    if single {
        capabilities.push("connection.single.v1");
    }
    let mut socket = connect(&address, &capabilities).await;
    let status = request(&mut socket, METHODS[0], json!({})).await;
    assert_eq!(status["result"]["state"], "up-to-date", "{status}");
    assert_eq!(
        status["result"]["available"],
        json!(["example", "orchestration"])
    );
    assert!(
        root.path()
            .join(".agents/skills/ait-orchestration/SKILL.md")
            .is_file()
    );
    let imported = request(
        &mut socket,
        METHODS[4],
        json!({"selection":{"mode":"custom","skills":[" example ","orchestration"]}}),
    )
    .await;
    assert_eq!(imported["result"]["imported"], true);
    let installed = request(&mut socket, METHODS[1], json!({})).await;
    assert_eq!(installed["result"]["state"], "up-to-date", "{installed}");
    for target in [".agents", ".claude", ".codex"] {
        assert_eq!(
            std::fs::read_to_string(root.path().join(target).join("skills/ait-example/SKILL.md"))
                .unwrap(),
            "example skill"
        );
    }
    let pending = request(
        &mut socket,
        METHODS[3],
        json!({"selection":{"mode":"custom","skills":[]}}),
    )
    .await;
    assert_eq!(
        pending["result"]["confirmationRequired"],
        json!({"removals":["example","orchestration"]})
    );
    let confirmed = request(
        &mut socket,
        METHODS[3],
        json!({"selection":{"mode":"custom","skills":[]},"confirmedRemovals":["example","orchestration"]}),
    )
    .await;
    assert_eq!(confirmed["result"]["state"], "not-installed");
    assert!(confirmed["result"]["confirmationRequired"].is_null());
    drop(socket);
    terminate(&mut process).await;
    let mut process = start(&data, &log);
    let address = ready(&mut process, &log).await;
    let mut socket = connect(&address, &capabilities).await;
    assert_eq!(
        request(&mut socket, METHODS[0], json!({})).await["result"]["selection"],
        json!({"mode":"custom","skills":[]})
    );
    assert!(
        !root
            .path()
            .join(".agents/skills/ait-orchestration")
            .exists()
    );
    assert_eq!(
        request(&mut socket, METHODS[4], json!({"selection":{"mode":"all"}})).await["result"]["imported"],
        false
    );
    assert_eq!(
        request(&mut socket, METHODS[3], json!({"selection":{"mode":"all"}})).await["result"]["state"],
        "up-to-date"
    );
    assert_eq!(
        request(&mut socket, METHODS[2], json!({})).await["result"]["state"],
        "not-installed"
    );
    assert_eq!(
        request(&mut socket, METHODS[0], json!({"agentId":"invalid"})).await["code"],
        "invalid_message"
    );
    drop(socket);
    terminate(&mut process).await;
    uninstall_stays_disabled_after_restart(&data, &log, &capabilities).await;
}

async fn uninstall_stays_disabled_after_restart(
    data: &std::path::Path,
    log: &std::path::Path,
    capabilities: &[&str],
) {
    let mut process = start(data, log);
    let address = ready(&mut process, log).await;
    let mut socket = connect(&address, capabilities).await;
    let status = request(&mut socket, METHODS[0], json!({})).await;
    assert_eq!(status["result"]["state"], "not-installed");
    assert_eq!(status["result"]["selection"], json!({"mode":"all"}));
    assert!(
        !data
            .parent()
            .unwrap()
            .join(".agents/skills/ait-orchestration")
            .exists()
    );
    assert_eq!(
        request(&mut socket, METHODS[1], json!({})).await["result"]["state"],
        "up-to-date"
    );
    drop(socket);
    terminate(&mut process).await;
}

#[tokio::test]
async fn embedded_skill_is_distributed_without_a_client_and_updated_after_restart() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("state");
    let log = root.path().join("server.log");
    let mut process = start(&data, &log);
    ready(&mut process, &log).await;
    for target in [".agents", ".claude", ".codex"] {
        let skill = root.path().join(target).join("skills/ait-orchestration");
        assert_eq!(
            std::fs::read_to_string(skill.join("SKILL.md")).unwrap(),
            include_str!("../../../../skills/ait-orchestration/SKILL.md")
        );
        assert!(skill.join("references/operations.md").is_file());
        assert!(skill.join("references/mcp-access.md").is_file());
        assert!(skill.join("agents/openai.yaml").is_file());
    }
    terminate(&mut process).await;
    let skill = root.path().join(".codex/skills/ait-orchestration");
    std::fs::write(skill.join("SKILL.md"), "old version").unwrap();
    std::fs::write(skill.join("personal.txt"), "keep").unwrap();
    std::fs::remove_file(skill.join("references/operations.md")).unwrap();

    let mut process = start(&data, &log);
    let address = ready(&mut process, &log).await;

    assert_eq!(
        std::fs::read_to_string(skill.join("SKILL.md")).unwrap(),
        include_str!("../../../../skills/ait-orchestration/SKILL.md")
    );
    assert!(skill.join("references/operations.md").is_file());
    assert_eq!(
        std::fs::read_to_string(skill.join("personal.txt")).unwrap(),
        "keep"
    );
    let mut socket = connect(&address, METHODS).await;
    assert_eq!(
        request(&mut socket, METHODS[0], json!({})).await["result"]["state"],
        "up-to-date"
    );
    drop(socket);
    terminate(&mut process).await;
}

#[tokio::test]
async fn foreign_skill_conflict_preserves_files_and_does_not_prevent_startup() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("state");
    let skill = root.path().join(".agents/skills/ait-orchestration");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(skill.join("SKILL.md"), "personal skill").unwrap();
    let log = root.path().join("server.log");

    let mut process = start(&data, &log);
    ready(&mut process, &log).await;

    assert_eq!(
        std::fs::read_to_string(skill.join("SKILL.md")).unwrap(),
        "personal skill"
    );
    assert!(
        !root
            .path()
            .join(".claude/skills/ait-orchestration")
            .exists()
    );
    assert!(
        std::fs::read_to_string(&log)
            .unwrap()
            .contains("automatic Agent skill distribution failed")
    );
    terminate(&mut process).await;
}
