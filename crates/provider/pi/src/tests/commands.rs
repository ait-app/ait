use super::*;

#[tokio::test]
async fn discovers_native_commands_without_creating_a_session() {
    let (directory, client, spec) = fixture("");

    let commands = client.commands(&spec).await.unwrap();

    assert_eq!(
        commands.as_slice(),
        &[
            json!({"name":"review","description":"","argumentHint":"","kind":"command"}),
            json!({"name":"fix-tests","description":"Fix tests","argumentHint":"","kind":"command"}),
            json!({"name":"skill:rust","description":"Rust guidance","argumentHint":"","kind":"skill"}),
        ]
    );
    let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
    let requests: Vec<Value> = requests
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["type"], "get_commands");
    let arguments: Vec<String> = serde_json::from_str(
        &std::fs::read_to_string(directory.path().join("arguments.json")).unwrap(),
    )
    .unwrap();
    assert!(arguments.iter().any(|argument| argument == "--no-session"));
    assert!(!directory.path().join("session.jsonl").exists());
}

#[tokio::test]
async fn command_discovery_propagates_native_and_catalog_failures() {
    for (scenario, expected) in [
        ("commands-rejected", AgentSessionError::Unavailable),
        ("commands-malformed", AgentSessionError::Failed),
    ] {
        let (_directory, client, spec) = fixture(scenario);
        assert_eq!(client.commands(&spec).await, Err(expected), "{scenario}");
    }
}

#[tokio::test]
async fn invalid_command_spec_is_rejected_before_spawning() {
    let (directory, client, mut spec) = fixture("");
    spec.provider = "other".into();
    assert_eq!(
        client.commands(&spec).await,
        Err(AgentSessionError::Rejected)
    );
    assert!(!directory.path().join("versions.log").exists());
}

#[test]
fn command_catalog_rejects_malformed_or_excessive_entries() {
    for response in [
        json!({}),
        json!({"commands":null}),
        json!({"commands":vec![json!({"name":"review"});4097]}),
    ] {
        assert_eq!(
            crate::commands::catalog(&response),
            Err(AgentSessionError::Failed)
        );
    }
    assert_eq!(
        crate::commands::catalog(&json!({"commands":[]})).unwrap(),
        Vec::<Value>::new()
    );
}

#[tokio::test]
async fn slash_invocations_preserve_native_names_and_arguments() {
    let (directory, client, spec) = fixture("");
    let commands = client.commands(&spec).await.unwrap();
    let mut session = client.create_session(&spec).await.unwrap();
    for (command, arguments) in commands
        .iter()
        .zip(["changes", "failing test", "ownership"])
    {
        let input = format!("/{} {arguments}", command["name"].as_str().unwrap());
        session.start_turn(&input, &spec.config).await.unwrap();
        let events = terminal(session.as_mut()).await;
        assert!(matches!(events.last(), Some(AgentTurnEvent::Completed(_))));
    }
    session.close().await.unwrap();
    let requests = std::fs::read_to_string(directory.path().join("requests.jsonl")).unwrap();
    let prompts: Vec<Value> = requests
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|request| request["type"] == "prompt")
        .map(|request| request["message"].clone())
        .collect();
    assert_eq!(
        prompts,
        [
            "/review changes",
            "/fix-tests failing test",
            "/skill:rust ownership"
        ]
    );
}

#[tokio::test]
async fn command_discovery_ignores_session_settings_but_validates_directory() {
    let (directory, client, mut spec) = fixture("");
    spec.config = StoredAgentConfig {
        model: Some("unqualified".into()),
        mode_id: Some("unsupported".into()),
        system_prompt: Some("unused".into()),
        ..StoredAgentConfig::default()
    };
    assert_eq!(client.commands(&spec).await.unwrap().len(), 3);
    spec.cwd = directory.path().join("missing").to_str().unwrap().into();
    assert_eq!(
        client.commands(&spec).await,
        Err(AgentSessionError::Rejected)
    );
}

#[tokio::test]
async fn repeated_command_requests_probe_again() {
    let (directory, client, spec) = fixture("");
    let first = client.commands(&spec).await.unwrap();
    assert_eq!(client.clone().commands(&spec).await.unwrap(), first);
    // A changed native result must be visible immediately, even after success.
    std::fs::write(directory.path().join("commands-rejected"), "").unwrap();
    assert_eq!(
        client.commands(&spec).await,
        Err(AgentSessionError::Unavailable)
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join("requests.jsonl"))
            .unwrap()
            .lines()
            .count(),
        3
    );
}

#[tokio::test]
async fn command_failures_can_recover_immediately() {
    let (directory, client, spec) = fixture("");
    let failure = directory.path().join("commands-rejected");
    std::fs::write(&failure, "").unwrap();
    assert_eq!(
        client.commands(&spec).await,
        Err(AgentSessionError::Unavailable)
    );
    std::fs::remove_file(failure).unwrap();
    assert_eq!(client.commands(&spec).await.unwrap().len(), 3);
}

#[test]
fn command_descriptions_and_escaped_catalog_payload_are_bounded() {
    let commands = crate::commands::catalog(&json!({"commands":[{
        "name":"review", "description": "é".repeat(513)
    }]}))
    .unwrap();
    assert_eq!(commands[0]["description"].as_str().unwrap().len(), 1024);
    let commands = crate::commands::catalog(&json!({"commands": (0..4096).map(|index| json!({
        "name":format!("command-{index}"), "description":"\"".repeat(1024)
    })).collect::<Vec<_>>()}))
    .unwrap();
    assert_eq!(commands.len(), 4096);
    assert!(commands.iter().all(|command| command["description"] == ""));
    assert!(serde_json::to_vec(&commands).unwrap().len() <= 768 * 1024);
    assert_eq!(
        crate::commands::catalog(&json!({"commands":(0..4096).map(|index| json!({
            "name":format!("{index:04}{}", "x".repeat(1020))
        })).collect::<Vec<_>>()})),
        Err(AgentSessionError::Failed)
    );
}

#[tokio::test]
async fn manager_returns_the_native_catalog() {
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
    use provider::service::agent_manager::AgentManager;
    use provider::testing::Manager;

    let (directory, client, spec) = fixture("");
    let registry = FileBackedAgentRuntimeRegistry::new(directory.path().join("agents.json"));
    let mut manager = Manager::new(AgentManager::new(Box::new(registry)));
    manager.register(Box::new(client.clone())).unwrap();
    let catalog = client.commands(&spec).await.unwrap();

    let mut response: Vec<Value> = manager.commands(&spec).await.unwrap();

    assert_eq!(catalog, response);
    response[0]["name"] = json!("changed");
    assert_eq!(client.commands(&spec).await.unwrap(), catalog);
}

#[tokio::test]
async fn blocked_directory_does_not_delay_other_directories() {
    let (directory, client, spec) = fixture("");
    let catalog = client.commands(&spec).await.unwrap();
    let blocked = directory.path().join("blocked");
    std::fs::create_dir(&blocked).unwrap();
    std::fs::write(blocked.join("commands-blocked"), "").unwrap();
    let mut blocked_spec = spec.clone();
    blocked_spec.cwd = blocked.to_str().unwrap().into();
    let cloned = client.clone();
    let probe = tokio::spawn(async move { cloned.commands(&blocked_spec).await });
    wait_for_command_probe(&blocked).await;

    assert_eq!(catalog, client.commands(&spec).await.unwrap());
    let other = directory.path().join("other");
    std::fs::create_dir(&other).unwrap();
    let mut other_spec = spec;
    other_spec.cwd = other.to_str().unwrap().into();
    assert_eq!(client.commands(&other_spec).await.unwrap().len(), 3);
    assert!(!probe.is_finished());

    std::fs::remove_file(blocked.join("commands-blocked")).unwrap();
    assert_eq!(probe.await.unwrap().unwrap().len(), 3);
}

async fn wait_for_command_probe(directory: &std::path::Path) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !directory.join("commands-started").exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn directory_aliases_discover_the_same_catalog() {
    let (directory, client, spec) = fixture("");
    let links = tempfile::tempdir().unwrap();
    let symlink = links.path().join("workspace");
    std::os::unix::fs::symlink(&spec.cwd, &symlink).unwrap();
    std::fs::create_dir(directory.path().join("child")).unwrap();
    let alias_paths = [
        symlink.to_str().unwrap().to_owned(),
        format!("{}/", spec.cwd),
        format!("{}/./child/..", spec.cwd),
    ];
    for alias in alias_paths {
        let (canonical, aliased) = tokio::join!(
            crate::commands::discover(&client, &spec.cwd, None),
            crate::commands::discover(&client, &alias, None)
        );
        assert_eq!(canonical.unwrap(), aliased.unwrap());
    }
    assert_eq!(
        std::fs::read_to_string(directory.path().join("requests.jsonl"))
            .unwrap()
            .lines()
            .count(),
        6
    );
    assert_eq!(
        crate::commands::discover(
            &client,
            directory.path().join("missing").to_str().unwrap(),
            None
        )
        .await,
        Err(AgentSessionError::Rejected)
    );
}

#[test]
fn command_catalog_omits_unrepresentable_names_without_renaming() {
    let names = [
        "",
        "bad\nname",
        "release notes",
        "path/name",
        "double\"quote",
        "single'quote",
        "non\u{a0}breaking",
        "bom\u{feff}space",
    ];
    let mut entries: Vec<Value> = names.into_iter().map(|name| json!({"name":name})).collect();
    entries.extend([
        json!({}),
        json!({"name":42}),
        json!({"name":"x".repeat(1025)}),
    ]);
    entries.push(json!({"name":"skill:rust", "source":"skill"}));

    let commands = crate::commands::catalog(&json!({"commands":entries})).unwrap();

    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0]["name"], "skill:rust");
    assert_eq!(commands[0]["kind"], "skill");
}

#[test]
fn command_catalog_keeps_first_duplicate_in_native_order() {
    let commands = crate::commands::catalog(&json!({"commands":[
        {"name":"review", "description":"Extension", "source":"extension"},
        {"name":"skill:rust", "source":"skill"},
        {"name":"review", "description":"Template", "source":"prompt"},
        {"name":"skill:rust", "source":"prompt"}
    ]}))
    .unwrap();

    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0]["name"], "review");
    assert_eq!(commands[0]["description"], "Extension");
    assert_eq!(commands[1]["name"], "skill:rust");
    assert_eq!(commands[1]["kind"], "skill");
}

#[test]
fn command_descriptions_replace_control_characters_with_spaces() {
    let commands = crate::commands::catalog(&json!({"commands":[
        {"name":"review", "description":"Line\nnext\r\tword\u{1b}\0\u{85}end"},
        {"name":"plain", "description":"Plain é text"}
    ]}))
    .unwrap();

    assert_eq!(commands[0]["description"], "Line next  word   end");
    assert_eq!(commands[1]["description"], "Plain é text");
}

#[test]
fn environment_scoped_session_clients_reset_the_version_cache() {
    let (_directory, client, _spec) = fixture("");
    let environment = AgentEnvironment::try_from(BTreeMap::from([(
        "PI_FIXTURE_SCENARIO".into(),
        "commands-rejected".into(),
    )]))
    .unwrap();

    let scoped = client.with_environment(&environment);

    assert!(!Arc::ptr_eq(&client.version, &scoped.version));
}

#[tokio::test]
async fn command_discovery_revalidates_removed_directories() {
    let (directory, client, mut spec) = fixture("");
    let cwd = directory.path().join("workspace");
    std::fs::create_dir(&cwd).unwrap();
    spec.cwd = cwd.to_str().unwrap().into();
    client.commands(&spec).await.unwrap();
    std::fs::remove_dir_all(&cwd).unwrap();

    assert_eq!(
        client.commands(&spec).await,
        Err(AgentSessionError::Rejected)
    );
}

#[tokio::test]
async fn command_discovery_rejects_relative_paths_and_files() {
    let (directory, client, mut spec) = fixture("");
    for cwd in [
        ".".to_owned(),
        directory.path().join("pi").to_str().unwrap().to_owned(),
    ] {
        spec.cwd = cwd;
        assert_eq!(
            client.commands(&spec).await,
            Err(AgentSessionError::Rejected)
        );
    }
    assert!(!directory.path().join("versions.log").exists());
}

#[tokio::test]
async fn wedged_command_discovery_times_out() {
    let capture = diagnostics::Capture::default();
    let _subscriber = tracing::subscriber::set_default(capture.subscriber());
    let (directory, client, spec) = fixture("");
    std::fs::write(directory.path().join("commands-blocked"), "").unwrap();
    let cloned = client.clone();
    let probe_spec = spec.clone();
    let probe = tokio::spawn(async move { cloned.commands(&probe_spec).await });
    wait_for_command_probe(directory.path()).await;
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(2)).await;
    tokio::time::resume();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), probe)
            .await
            .unwrap()
            .unwrap(),
        Err(AgentSessionError::Failed)
    );

    let logs = capture.text();
    assert!(
        logs.contains("Pi discovery RPC request timed out"),
        "{logs}"
    );
    assert!(logs.contains("2s"), "{logs}");
    assert!(!logs.contains("cleanup failed"), "{logs}");
}

#[tokio::test]
async fn cold_command_discovery_allows_slow_version_and_caches_it() {
    let (directory, client, mut spec) = fixture("version-slow");

    assert_eq!(client.commands(&spec).await.unwrap().len(), 3);
    let other = directory.path().join("other");
    std::fs::create_dir(&other).unwrap();
    spec.cwd = other.to_str().unwrap().into();
    assert_eq!(client.commands(&spec).await.unwrap().len(), 3);
    assert_eq!(
        std::fs::read_to_string(directory.path().join("versions.log")).unwrap(),
        "probe\n"
    );
}

#[tokio::test]
async fn cold_command_discovery_records_version_timeout_cooldown() {
    let (directory, mut client, spec) = fixture("version-timeout");
    client.deadline = Duration::from_secs(30);

    let result = tokio::time::timeout(Duration::from_secs(7), client.commands(&spec)).await;

    assert_eq!(result.unwrap(), Err(AgentSessionError::Failed));
    // Verify the shared version cooldown was recorded.
    assert_eq!(
        transport::version(&client).await,
        Err(AgentSessionError::Failed)
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join("versions.log")).unwrap(),
        "probe\n"
    );
    assert!(!directory.path().join("requests.jsonl").exists());
}

#[tokio::test]
async fn timed_out_command_probe_recovers_on_first_client_retry() {
    let (directory, client, spec) = fixture("");
    let blocked = directory.path().join("commands-blocked");
    std::fs::write(&blocked, "").unwrap();
    assert!(client.commands(&spec).await.is_err());
    assert!(directory.path().join("commands-started").exists());
    std::fs::remove_file(blocked).unwrap();
    // Model the client's first retry without sleeping across a real process boundary.
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(1)).await;
    tokio::time::resume();
    assert_eq!(client.commands(&spec).await.unwrap().len(), 3);
}

#[tokio::test]
async fn command_discovery_honors_confinement_on_every_request() {
    let (_directory, client, mut spec) = fixture("");
    client.commands(&spec).await.unwrap();
    // An invalid sandbox root must fail closed after successful unconfined discovery.
    let workspace = serde_json::from_value(json!({
        "workspaceId": "wks_00000000000000a1",
        "projectId": "prj_1",
        "cwd": spec.cwd,
        "kind": "worktree",
        "displayName": "feature",
        "worktreeRoot": "/nonexistent-pi-discovery-sandbox",
        "createdAt": "2026-01-01T00:00:00Z",
        "updatedAt": "2026-01-01T00:00:00Z",
        "archivedAt": null,
        "sandbox": "worktree_writes"
    }))
    .unwrap();
    spec.confinement = domain::workspace::sandbox::WriteConfinement::for_workspace(&workspace);

    assert!(client.commands(&spec).await.is_err());
    spec.confinement = None;
    assert!(client.commands(&spec).await.is_ok());
}

#[tokio::test]
async fn old_pi_command_retries_reuse_version_without_spawning_rpc() {
    let (directory, client, spec) = fixture("old");
    for _ in 0..8 {
        assert_eq!(
            client.clone().commands(&spec).await,
            Err(AgentSessionError::Unavailable)
        );
    }
    assert_eq!(
        std::fs::read_to_string(directory.path().join("versions.log"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    assert!(!directory.path().join("requests.jsonl").exists());
    assert!(!directory.path().join("arguments.json").exists());
}
