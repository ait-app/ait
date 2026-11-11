use super::*;
use crate::local::omp::aliases;
use crate::ports::agent_session::{AgentClient, AgentResumePurpose};
use domain::workspace::records::PersistedWorkspaceRecord;
use domain::workspace::sandbox::WriteConfinement;
use std::path::Path;

fn confinement(root: &Path) -> WriteConfinement {
    let record: PersistedWorkspaceRecord = serde_json::from_value(json!({
        "workspaceId":"wks_0123456789abcdef", "projectId":"prj_boundary",
        "cwd":root, "kind":"worktree", "displayName":"boundary",
        "worktreeRoot":root, "createdAt":"2026-11-11T00:00:00Z",
        "updatedAt":"2026-11-11T00:00:00Z", "archivedAt":null,
        "sandbox":"worktree_writes"
    }))
    .unwrap();
    WriteConfinement::for_workspace(&record).unwrap()
}

#[tokio::test]
async fn native_restore_and_history_resolve_legacy_alias_without_rewriting_records() {
    let (root, client, spec) = fixture();
    let mut original = client.create_session(&spec).await.unwrap();
    original.start_turn("hello", &spec.config).await.unwrap();
    complete(original.as_mut()).await;
    let native = original.persistence().unwrap();
    original.close().await.unwrap();
    let journal = native.metadata.as_ref().unwrap()["sessionFile"]
        .as_str()
        .unwrap();
    let before = std::fs::read(journal).unwrap();
    let id = Uuid::new_v4().to_string();
    aliases::save(
        &client,
        &id,
        &aliases::Alias {
            cwd: spec.cwd.clone(),
            handle: native.clone(),
            config: StoredAgentConfig {
                mode_id: Some("write".to_owned()),
                ..StoredAgentConfig::default()
            },
        },
    )
    .unwrap();
    let alias_path = root
        .path()
        .join("agent/ait-acp-aliases")
        .join(format!("{id}.json"));
    let alias_before = std::fs::read(&alias_path).unwrap();
    let legacy = AgentPersistenceHandle {
        provider: PROVIDER.to_owned(),
        session_id: id,
        native_handle: None,
        metadata: Some(BTreeMap::from([("transport".to_owned(), json!("acp"))])),
    };

    let history = client.history(&legacy, &spec.cwd).await.unwrap();
    assert!(history.iter().any(|item| item.item["text"] == "hello"));
    let mut restored = open(&client, &spec, Some(&legacy)).await.unwrap();
    assert_eq!(
        restored.persistence().unwrap().session_id,
        native.session_id
    );
    assert_eq!(
        restored.config, spec.config,
        "caller configuration remains authoritative"
    );
    restored.close().await.unwrap();
    assert_eq!(std::fs::read(journal).unwrap(), before);
    assert_eq!(std::fs::read(&alias_path).unwrap(), alias_before);
}

#[tokio::test]
async fn explicit_native_file_identity_does_not_follow_an_unrelated_alias() {
    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("hello", &spec.config).await.unwrap();
    complete(session.as_mut()).await;
    let native = session.persistence().unwrap();
    session.close().await.unwrap();
    let mut unrelated = native.clone();
    unrelated.session_id = Uuid::new_v4().to_string();
    aliases::save(
        &client,
        &native.session_id,
        &aliases::Alias {
            cwd: spec.cwd.clone(),
            handle: unrelated,
            config: StoredAgentConfig::default(),
        },
    )
    .unwrap();

    let mut restored = client
        .resume_session(&native, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    assert_eq!(
        restored.persistence().unwrap().session_id,
        native.session_id
    );
    restored.close().await.unwrap();
}

#[tokio::test]
async fn direct_native_create_and_scoped_probes_refuse_unresolvable_confinement() {
    let (root, client, mut spec) = fixture();
    spec.confinement = Some(confinement(&root.path().join("missing-worktree")));

    assert!(client.create_session(&spec).await.is_err());
    assert!(client.draft_features(&spec).await.is_err());
    assert!(client.commands(&spec).await.is_err());
    assert!(!root.path().join("agent/calls.jsonl").exists());
}

#[tokio::test]
async fn native_restore_keeps_the_callers_confinement_instead_of_alias_configuration() {
    let (root, client, mut spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("hello", &spec.config).await.unwrap();
    complete(session.as_mut()).await;
    let native = session.persistence().unwrap();
    session.close().await.unwrap();
    let id = Uuid::new_v4().to_string();
    aliases::save(
        &client,
        &id,
        &aliases::Alias {
            cwd: spec.cwd.clone(),
            handle: native,
            config: spec.config.clone(),
        },
    )
    .unwrap();
    let legacy = AgentPersistenceHandle {
        provider: PROVIDER.to_owned(),
        session_id: id,
        native_handle: None,
        metadata: None,
    };
    let calls = root.path().join("agent/calls.jsonl");
    let before = std::fs::read(&calls).unwrap();
    spec.confinement = Some(confinement(&root.path().join("missing-worktree")));

    assert!(
        client
            .resume_session(&legacy, &spec, AgentResumePurpose::Interactive)
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read(calls).unwrap(),
        before,
        "no unconfined native process was started"
    );
}

#[tokio::test]
async fn config_reopen_fails_closed_when_recorded_confinement_becomes_unresolvable() {
    let (root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    // Model a previously valid placement becoming unavailable after launch. Reopen must
    // retain this exact placement rather than infer an unrestricted policy from cwd.
    let recorded = confinement(&root.path().join("removed-worktree"));
    session.confinement = Some(recorded.clone());
    let config = StoredAgentConfig {
        mode_id: Some("write".to_owned()),
        ..spec.config.clone()
    };
    let calls = root.path().join("agent/calls.jsonl");
    let before = std::fs::read(&calls).unwrap();

    assert!(session.configure(&config).await.is_err());
    assert_eq!(session.confinement, Some(recorded));
    assert_eq!(std::fs::read(calls).unwrap(), before);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
#[ignore = "requires functional OS write sandbox; exercises native create, restore and config reopen"]
async fn native_sandbox_placement_survives_create_restore_and_config_reopen() {
    let (root, mut client, mut spec) = fixture();
    let worktree = root.path().join("worktree");
    let private = root.path().join("repo/.git/worktrees/worktree");
    std::fs::create_dir_all(&private).unwrap();
    std::fs::write(private.join("commondir"), "../..\n").unwrap();
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::write(
        worktree.join(".git"),
        format!("gitdir: {}\n", private.display()),
    )
    .unwrap();
    client.environment = crate::ports::environment::AgentEnvironment::try_from(BTreeMap::from([
        ("HOME".to_owned(), root.path().to_str().unwrap().to_owned()),
        (
            "PI_CODING_AGENT_DIR".to_owned(),
            worktree.join("agent").to_str().unwrap().to_owned(),
        ),
    ]))
    .unwrap();
    spec.cwd = worktree.to_str().unwrap().to_owned();
    spec.confinement = Some(confinement(&worktree));
    let mut session = open(&client, &spec, None).await.unwrap();
    assert_eq!(session.confinement, spec.confinement);
    session.start_turn("hello", &spec.config).await.unwrap();
    complete(&mut session).await;
    let native = session.persistence().unwrap();
    session.close().await.unwrap();

    let mut restored = open(&client, &spec, Some(&native)).await.unwrap();
    assert_eq!(restored.confinement, spec.confinement);
    let config = StoredAgentConfig {
        mode_id: Some("write".to_owned()),
        ..spec.config.clone()
    };
    restored.configure(&config).await.unwrap();
    assert_eq!(restored.confinement, spec.confinement);
    restored.start_turn("after reopen", &config).await.unwrap();
    complete(&mut restored).await;
    restored.close().await.unwrap();
}

/// A role assignment reaches OMP only through the private `--config` overlay a fresh process
/// reads at startup, so the adapter must reopen instead of using a live setter.
#[tokio::test]
async fn role_assignment_reopens_with_a_private_overlay_and_keeps_the_session() {
    let (root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    session.start_turn("hello", &spec.config).await.unwrap();
    complete(&mut session).await;
    let before = session.state["sessionId"].clone();
    let session_file = session.state["sessionFile"].as_str().unwrap().to_owned();
    let calls = root.path().join("agent/calls.jsonl");
    let launched = std::fs::read_to_string(&calls).unwrap().lines().count();
    let config = StoredAgentConfig {
        feature_values: Some(BTreeMap::from([(
            "modelRole.task".to_owned(),
            json!("test/model:high"),
        )])),
        ..spec.config.clone()
    };

    session.configure(&config).await.unwrap();

    assert_eq!(
        session.state["sessionId"], before,
        "same native session resumed"
    );
    assert_eq!(session.config, config);
    let launch = std::fs::read_to_string(&calls)
        .unwrap()
        .lines()
        .skip(launched)
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|call| {
            call["args"]
                .as_array()
                .is_some_and(|args| args.iter().any(|arg| arg == "--config"))
        })
        .expect("a new native process was launched with the private overlay");
    let args: Vec<&str> = launch["args"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let overlay = args
        .windows(2)
        .find(|pair| pair[0] == "--config")
        .map(|pair| pair[1])
        .expect("the overlay path is passed on the command line");
    assert!(
        args.windows(2)
            .any(|pair| pair[0] == "--resume" && pair[1] == session_file.as_str()),
        "reopen resumes the persisted session"
    );
    assert_eq!(
        serde_json::from_str::<Value>(&std::fs::read_to_string(overlay).unwrap()).unwrap(),
        json!({"modelRoles":{"task":"test/model:high"}})
    );
    assert!(
        Path::new(overlay).exists(),
        "the live process keeps its overlay readable"
    );
    session.close().await.unwrap();
    drop(session);
    assert!(
        !Path::new(overlay).exists(),
        "the overlay dies with the native process"
    );
}

/// Role assignments the live catalog cannot resolve stay display-only: an apply that would
/// write them is rejected, and inheriting remains the escape hatch.
#[tokio::test]
async fn an_unresolvable_role_assignment_is_rejected_but_inherit_still_applies() {
    let (root, client, spec) = fixture();
    let mut session = open(&client, &spec, None).await.unwrap();
    let unknown = StoredAgentConfig {
        feature_values: Some(BTreeMap::from([(
            "modelRole.ghost".to_owned(),
            json!("test/model"),
        )])),
        ..spec.config.clone()
    };
    assert_eq!(
        session.configure(&unknown).await.unwrap_err(),
        AgentSessionError::Rejected
    );
    let inherit = StoredAgentConfig {
        feature_values: Some(BTreeMap::from([(
            "modelRole.ghost".to_owned(),
            json!("native"),
        )])),
        ..spec.config.clone()
    };
    session.configure(&inherit).await.unwrap();
    assert_eq!(session.config, inherit);
    assert!(
        !root.path().join("agent/calls.jsonl").exists()
            || !std::fs::read_to_string(root.path().join("agent/calls.jsonl"))
                .unwrap()
                .contains("ghost"),
        "no unresolved assignment reached the native process"
    );
    session.close().await.unwrap();
}
