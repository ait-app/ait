use super::*;
use crate::local::omp::aliases;
use domain::workspace::{records::PersistedWorkspaceRecord, sandbox::WriteConfinement};

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; exercises native lifecycle using loopback-only model fixture"]
async fn installed_native_lifecycle_keeps_legacy_aliases_and_append_only_history() {
    lifecycle(false).await;
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN and functional OS sandbox; uses loopback-only model fixture"]
async fn installed_native_create_restore_and_config_reopen_preserve_write_confinement() {
    lifecycle(true).await;
}

async fn lifecycle(confined: bool) {
    let mut fixture = installed_with_plugins().await;
    let (_repository, outside) = prepare_fixture(&mut fixture, confined);
    let root = fixture.root.path();
    let agent = root.join("agent");
    let client = &fixture.client;
    let spec = &mut fixture.spec;
    let mut session = client.create_session(spec).await.unwrap();
    check_writes(session.as_mut(), spec, confined).await;
    session
        .start_turn(
            "Persist the boundary fixture using the local test model.",
            &spec.config,
        )
        .await
        .unwrap();
    let events = complete(session.as_mut()).await;
    assert!(
        matches!(events.last(), Some(AgentTurnEvent::Completed(_))),
        "{events:?}"
    );
    let native = session.persistence().unwrap();
    session.close().await.unwrap();
    let journal = native.metadata.as_ref().unwrap()["sessionFile"]
        .as_str()
        .unwrap();
    let original = std::fs::read(journal).unwrap();
    assert!(!original.is_empty());
    let header = String::from_utf8_lossy(&original)
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|record| record["type"] == "session")
        .unwrap();
    assert_eq!(
        header["id"], native.session_id,
        "persisted native identity changed"
    );
    assert_eq!(
        std::fs::canonicalize(header["cwd"].as_str().unwrap()).unwrap(),
        std::fs::canonicalize(&spec.cwd).unwrap(),
        "persisted workspace changed"
    );

    let id = uuid::Uuid::new_v4().to_string();
    aliases::save(
        client,
        &id,
        &aliases::Alias {
            cwd: spec.cwd.clone(),
            handle: native.clone(),
            config: spec.config.clone(),
        },
    )
    .unwrap();
    let legacy = AgentPersistenceHandle {
        provider: "omp".to_owned(),
        session_id: id.clone(),
        native_handle: None,
        metadata: Some(BTreeMap::from([("transport".to_owned(), json!("acp"))])),
    };
    let alias_path = agent.join("ait-acp-aliases").join(format!("{id}.json"));
    let alias_before = std::fs::read(&alias_path).unwrap();
    assert!(!client.history(&legacy, &spec.cwd).await.unwrap().is_empty());
    let mut restored = client
        .resume_session(&legacy, spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    assert_eq!(
        restored.persistence().unwrap().session_id,
        native.session_id
    );
    check_writes(restored.as_mut(), spec, confined).await;
    spec.config.mode_id = Some("write".to_owned());
    check_writes(restored.as_mut(), spec, confined).await;
    assert_eq!(
        restored.persistence().unwrap().session_id,
        native.session_id
    );
    assert_eq!(
        restored.runtime_info().await.unwrap().mode_id.as_deref(),
        Some("write")
    );
    restored.close().await.unwrap();

    assert!(
        std::fs::read(journal).unwrap().starts_with(&original),
        "native log was rewritten rather than appended"
    );
    assert_eq!(std::fs::read(alias_path).unwrap(), alias_before);
    assert_eq!(outside.path().join("probe.txt").exists(), !confined);
    assert_eq!(
        std::fs::read_to_string(root.join("boundary-inside.txt")).unwrap(),
        "inside"
    );
}

fn prepare_fixture(
    fixture: &mut Installed,
    confined: bool,
) -> (tempfile::TempDir, tempfile::TempDir) {
    let repository = tempfile::tempdir().unwrap();
    // Both OS policies deliberately permit scratch writes; the denied probe must be outside /tmp.
    let outside = tempfile::Builder::new()
        .prefix(".omp-boundary-outside-")
        .tempdir_in(env!("CARGO_MANIFEST_DIR"))
        .unwrap();
    let root = fixture.root.path();
    let agent = root.join("agent");
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        agent.join("extensions/boundary-extension.ts"),
        include_str!("../fixtures/boundary-extension.ts"),
    )
    .unwrap();
    fixture.client.environment = AgentEnvironment::try_from(BTreeMap::from([
        ("HOME".to_owned(), home.to_str().unwrap().to_owned()),
        (
            "PI_CODING_AGENT_DIR".to_owned(),
            agent.to_str().unwrap().to_owned(),
        ),
        ("OMP_PROFILE".to_owned(), String::new()),
        ("PI_PROFILE".to_owned(), String::new()),
        (
            "OMP_BOUNDARY_INSIDE".to_owned(),
            root.join("boundary-inside.txt")
                .to_str()
                .unwrap()
                .to_owned(),
        ),
        (
            "OMP_BOUNDARY_OUTSIDE".to_owned(),
            outside
                .path()
                .join("probe.txt")
                .to_str()
                .unwrap()
                .to_owned(),
        ),
    ]))
    .unwrap();
    if confined {
        let private = repository.path().join(".git/worktrees/boundary");
        std::fs::create_dir_all(&private).unwrap();
        std::fs::write(private.join("commondir"), "../..\n").unwrap();
        std::fs::write(
            root.join(".git"),
            format!("gitdir: {}\n", private.display()),
        )
        .unwrap();
        let record: PersistedWorkspaceRecord = serde_json::from_value(json!({
            "workspaceId":"wks_0123456789abcdef", "projectId":"prj_boundary",
            "cwd":root, "kind":"worktree", "displayName":"boundary", "worktreeRoot":root,
            "createdAt":"2026-11-11T00:00:00Z", "updatedAt":"2026-11-11T00:00:00Z",
            "archivedAt":null, "sandbox":"worktree_writes"
        }))
        .unwrap();
        fixture.spec.confinement = WriteConfinement::for_workspace(&record);
    }
    (repository, outside)
}

async fn check_writes(session: &mut dyn AgentSession, spec: &AgentSessionSpec, confined: bool) {
    session
        .start_turn("/boundary-write", &spec.config)
        .await
        .unwrap();
    let events = complete(session).await;
    assert!(
        matches!(events.last(), Some(AgentTurnEvent::Completed(_))),
        "{events:?}"
    );
    assert!(events.iter().any(|event| matches!(event, AgentTurnEvent::Timeline(entry)
        if entry.item["text"].as_str().is_some_and(|text| {
            if confined {
                ["EROFS", "EPERM", "EACCES"].iter().any(|code| text == format!("boundary:inside=ok,outside={code}"))
            } else {
                text == "boundary:inside=ok,outside=ok"
            }
        }))), "{events:?}");
}
