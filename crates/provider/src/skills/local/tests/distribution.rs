use super::*;

#[test]
fn embedded_skill_is_distributed_repaired_and_keeps_user_files() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let mut service = Skills::new(Box::new(store(root).with_bundled()));

    service.distribute().unwrap();

    let status = call(&mut service, protocol::GET_STATUS, json!({}));
    assert_eq!(status["available"], json!(["orchestration"]));
    assert_eq!(status["installed"], json!(["orchestration"]));
    assert_eq!(status["state"], "up-to-date");
    for target in ["agents", "claude", "codex"] {
        let skill = root.join(target).join("ait-orchestration");
        assert!(
            fs::read_to_string(skill.join("SKILL.md"))
                .unwrap()
                .contains("name: ait-orchestration")
        );
        for resource in [
            "agents/openai.yaml",
            "references/operations.md",
            "references/mcp-access.md",
        ] {
            assert!(skill.join(resource).is_file(), "{resource}");
        }
        assert!(tree::owned_path(&skill.canonicalize().unwrap()).unwrap());
        assert!(!root.join(target).join("ait-ait-orchestration").exists());
    }
    assert!(!root.join("bundle").exists());
    assert!(!root.join("state/selection.json").exists());
    assert!(!root.join("state/automatic.json").exists());
    put(&root.join("agents/ait-orchestration/user.txt"), "keep");
    put(
        &root.join("codex/ait-orchestration/SKILL.md"),
        "old release",
    );
    fs::remove_file(root.join("claude/ait-orchestration/references/operations.md")).unwrap();
    drop(service);
    let mut service = Skills::new(Box::new(store(root).with_bundled()));

    service.distribute().unwrap();
    service.distribute().unwrap();

    assert_eq!(
        call(&mut service, protocol::GET_STATUS, json!({}))["state"],
        "up-to-date"
    );
    assert_eq!(
        fs::read_to_string(root.join("agents/ait-orchestration/user.txt")).unwrap(),
        "keep"
    );
    assert!(!root.join("state/transaction.json").exists());
}

#[test]
fn external_bundle_overrides_embedded_skill_and_all_follows_new_entries() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    put(&root.join("bundle/orchestration/SKILL.md"), "host override");
    let mut service = Skills::new(Box::new(store(root).with_bundled()));
    service.distribute().unwrap();
    assert_eq!(
        fs::read_to_string(root.join("agents/ait-orchestration/SKILL.md")).unwrap(),
        "host override"
    );
    put(&root.join("bundle/new-skill/SKILL.md"), "new");
    service.distribute().unwrap();
    assert_eq!(
        call(&mut service, protocol::GET_STATUS, json!({}))["installed"],
        json!(["new-skill", "orchestration"])
    );
    fs::remove_dir_all(root.join("bundle/orchestration")).unwrap();
    service.distribute().unwrap();
    assert!(
        fs::read_to_string(root.join("agents/ait-orchestration/SKILL.md"))
            .unwrap()
            .contains("name: ait-orchestration")
    );
}

#[test]
fn startup_respects_custom_selection_and_leaves_pending_removals() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    put(&root.join("bundle/selected/SKILL.md"), "selected");
    let mut store = store(root).with_bundled();
    store
        .import(&Selection::Custom {
            skills: vec!["selected".into(), "unknown".into()],
        })
        .unwrap();
    put(&root.join("agents/ait-retired/user.txt"), "keep");
    mark_owned(&root.join("agents/ait-retired"));
    let mut service = Skills::new(Box::new(store));

    service.distribute().unwrap();

    let status = call(&mut service, protocol::GET_STATUS, json!({}));
    assert_eq!(status["installed"], json!(["retired", "selected"]));
    assert_eq!(status["ops"], json!([{"kind":"delete","name":"retired"}]));
    assert!(!root.join("agents/ait-orchestration").exists());
    assert!(root.join("agents/ait-retired/user.txt").exists());
    call(
        &mut service,
        protocol::SAVE_SELECTION,
        json!({"selection":{"mode":"custom","skills":[]},"confirmedRemovals":["retired","selected"]}),
    );
    service.distribute().unwrap();
    assert_eq!(
        call(&mut service, protocol::GET_STATUS, json!({}))["installed"],
        json!([])
    );
}

#[test]
fn explicit_uninstall_survives_restart_and_manual_actions_resume_distribution() {
    for resume_with_save in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let mut service = Skills::new(Box::new(store(root).with_bundled()));
        service.distribute().unwrap();
        let removed = call(&mut service, protocol::UNINSTALL, json!({}));
        assert_eq!(removed["selection"], json!({"mode":"all"}));
        drop(service);
        let mut service = Skills::new(Box::new(store(root).with_bundled()));

        service.distribute().unwrap();

        assert!(!root.join("agents/ait-orchestration").exists());
        assert_eq!(
            call(&mut service, protocol::GET_STATUS, json!({}))["state"],
            "not-installed"
        );
        if resume_with_save {
            call(
                &mut service,
                protocol::SAVE_SELECTION,
                json!({"selection":{"mode":"all"}}),
            );
        } else {
            call(&mut service, protocol::RECONCILE, json!({}));
        }
        assert!(store(root).automatic().unwrap());
        fs::remove_dir_all(root.join("codex/ait-orchestration")).unwrap();
        service.distribute().unwrap();
        assert!(root.join("codex/ait-orchestration/SKILL.md").exists());
    }
}

#[test]
fn startup_preserves_foreign_skill_and_reports_invalid_automatic_state() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    put(
        &root.join("agents/ait-orchestration/SKILL.md"),
        "personal skill",
    );
    let mut service = Skills::new(Box::new(store(root).with_bundled()));
    assert_eq!(service.distribute(), Err(ErrorCode::ResourceExhausted));
    assert_eq!(
        fs::read_to_string(root.join("agents/ait-orchestration/SKILL.md")).unwrap(),
        "personal skill"
    );
    assert!(!root.join("claude/ait-orchestration").exists());
    put(&root.join("state/automatic.json"), "invalid");
    assert_eq!(service.distribute(), Err(ErrorCode::RegistryIo));
}
