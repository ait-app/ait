use std::collections::BTreeMap;

use provider_core::ports::environment::AgentEnvironment;

use super::*;

/// A client whose OMP environment is exactly `variables` over an isolated home, so the test
/// process's own OMP variables never apply.
fn client(home: &Path, variables: &[(&str, String)]) -> OmpClient {
    let mut values: BTreeMap<String, String> = [
        "PI_CODING_AGENT_DIR",
        "PI_CODING_AGENT_SESSION_DIR",
        "PI_CONFIG_DIR",
        "OMP_PROFILE",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "XDG_CACHE_HOME",
    ]
    .into_iter()
    .map(|key| (key.to_owned(), String::new()))
    .collect();
    values.insert("HOME".to_owned(), home.to_string_lossy().into_owned());
    values.extend(
        variables
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone())),
    );
    let mut client = OmpClient::new("unused".into());
    client.environment = AgentEnvironment::try_from(values).unwrap();
    client
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[test]
fn the_default_profile_and_configured_agent_directory_hold_sessions_and_blobs() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    let default = client(home, &[]);
    assert_eq!(
        sessions(&default, None).unwrap(),
        home.join(".omp/agent/sessions")
    );
    assert_eq!(
        blobs(&default, None).unwrap(),
        home.join(".omp/agent/blobs")
    );

    for profile in ["default", " default ", ""] {
        let named = client(home, &[("OMP_PROFILE", profile.to_owned())]);
        assert_eq!(
            sessions(&named, None).unwrap(),
            home.join(".omp/agent/sessions")
        );
    }
    // OMP_PROFILE, even empty, takes precedence over PI_PROFILE.
    let empty = client(
        home,
        &[
            ("OMP_PROFILE", String::new()),
            ("PI_PROFILE", "work".to_owned()),
        ],
    );
    assert_eq!(
        sessions(&empty, None).unwrap(),
        home.join(".omp/agent/sessions")
    );

    let profiled = client(
        home,
        &[
            ("OMP_PROFILE", "work.2".to_owned()),
            ("PI_CODING_AGENT_DIR", text(&home.join("ignored"))),
            ("PI_CONFIG_DIR", ".config/omp".to_owned()),
        ],
    );
    assert_eq!(
        sessions(&profiled, None).unwrap(),
        home.join(".config/omp/profiles/work.2/agent/sessions")
    );

    let agent = client(home, &[("PI_CODING_AGENT_DIR", "~/agent".to_owned())]);
    assert_eq!(sessions(&agent, None).unwrap(), home.join("agent/sessions"));
    let relative = client(home, &[("PI_CODING_AGENT_DIR", "agent".to_owned())]);
    assert_eq!(
        sessions(&relative, Some("/work")).unwrap(),
        Path::new("/work/agent/sessions")
    );
}

#[test]
fn profiles_follow_omps_character_rules() {
    for valid in ["a", "work", "w0rk.v2_x-y", &"a".repeat(64)] {
        assert!(valid_profile(valid), "{valid}");
    }
    for invalid in [
        "Work",
        ".",
        "..",
        "work.",
        "-work",
        "a/b",
        "nul",
        "COM1",
        "lpt9.txt",
        &"a".repeat(65),
    ] {
        assert!(!valid_profile(invalid), "{invalid}");
    }
    // OMP 18.18.8 refuses to start with an invalid selected profile, so there is no default
    // directory to fall back to.
    let home = tempfile::tempdir().unwrap();
    let invalid = client(home.path(), &[("OMP_PROFILE", "Bad Name".to_owned())]);
    assert!(matches!(
        sessions(&invalid, None),
        Err(AgentSessionError::Rejected)
    ));
    // An OMP_PROFILE, even empty, shadows an invalid PI_PROFILE, as OMP starts then.
    for selected in ["", "work"] {
        let shadowed = client(
            home.path(),
            &[
                ("OMP_PROFILE", selected.to_owned()),
                ("PI_PROFILE", "Bad Name".to_owned()),
            ],
        );
        assert!(sessions(&shadowed, None).is_ok(), "{selected:?}");
    }
}

#[test]
fn an_existing_xdg_data_root_holds_default_sessions_and_blobs() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    let xdg = home.join("xdg");
    let data = client(home, &[("XDG_DATA_HOME", text(&xdg))]);
    // A missing XDG root keeps the agent directory.
    assert_eq!(
        sessions(&data, None).unwrap(),
        home.join(".omp/agent/sessions")
    );

    std::fs::create_dir_all(xdg.join("omp/profiles/work")).unwrap();
    assert_eq!(sessions(&data, None).unwrap(), xdg.join("omp/sessions"));
    assert_eq!(blobs(&data, None).unwrap(), xdg.join("omp/blobs"));
    let profiled = client(
        home,
        &[
            ("XDG_DATA_HOME", text(&xdg)),
            ("OMP_PROFILE", "work".to_owned()),
        ],
    );
    assert_eq!(
        sessions(&profiled, None).unwrap(),
        xdg.join("omp/profiles/work/sessions")
    );
    // A configured agent directory is not replaced by XDG roots.
    let agent = client(
        home,
        &[
            ("XDG_DATA_HOME", text(&xdg)),
            ("PI_CODING_AGENT_DIR", text(&home.join("agent"))),
        ],
    );
    assert_eq!(sessions(&agent, None).unwrap(), home.join("agent/sessions"));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn diagnostic_roots_follow_the_profile_agent_directory_and_existing_xdg_roots() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    let xdg = |kind: &str| home.join(kind);
    let variables = |profile: &str, agent: Option<&Path>| {
        let mut variables = vec![
            ("OMP_PROFILE", profile.to_owned()),
            ("PI_CONFIG_DIR", "config".to_owned()),
            ("XDG_DATA_HOME", text(&xdg("data"))),
            ("XDG_STATE_HOME", text(&xdg("state"))),
            ("XDG_CACHE_HOME", text(&xdg("cache"))),
        ];
        if let Some(agent) = agent {
            variables.push(("PI_CODING_AGENT_DIR", text(agent)));
        }
        variables
    };

    let found = roots(&client(home, &variables("", None))).unwrap();
    assert_eq!(found.profile, None);
    assert_eq!(found.config, home.join("config"));
    assert_eq!(found.agent, home.join("config/agent"));
    assert_eq!(found.xdg, [None, None, None]);
    assert_eq!(found.database(), home.join("config/agent/agent.db"));

    for kind in ["data", "cache"] {
        std::fs::create_dir_all(xdg(kind).join("omp/profiles/work")).unwrap();
    }
    let found = roots(&client(home, &variables("work", None))).unwrap();
    assert_eq!(found.profile.as_deref(), Some("work"));
    assert_eq!(found.config, home.join("config/profiles/work"));
    assert_eq!(found.agent, home.join("config/profiles/work/agent"));
    assert_eq!(
        found.xdg,
        [
            Some(xdg("data").join("omp/profiles/work")),
            None,
            Some(xdg("cache").join("omp/profiles/work")),
        ]
    );
    assert_eq!(
        found.database(),
        xdg("data").join("omp/profiles/work/agent.db")
    );

    // A configured agent directory keeps the configuration root and ignores XDG roots.
    let agent = home.join("agent");
    let found = roots(&client(home, &variables("", Some(&agent)))).unwrap();
    assert_eq!((found.config, found.agent), (home.join("config"), agent));
    assert_eq!(found.xdg, [None, None, None]);
    assert!(roots(&client(home, &variables("Bad", None))).is_err());
}

#[test]
fn only_the_environment_moves_sessions_and_never_blobs() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    let agent = home.join("agent");
    let workspace = home.join("workspace");
    std::fs::create_dir_all(workspace.join(".omp")).unwrap();
    std::fs::create_dir_all(&agent).unwrap();
    let base = [("PI_CODING_AGENT_DIR", text(&agent))];
    let cwd = workspace.to_str();
    // OMP 18.18.8 ignores `sessionDir` in either settings file and writes to the agent's
    // `sessions` (verified with a probe holding both).
    std::fs::write(
        agent.join("settings.json"),
        r#"{"sessionDir":"~/agent-sessions"}"#,
    )
    .unwrap();
    std::fs::write(
        workspace.join(".omp/settings.json"),
        r#"{"sessionDir":"local-sessions"}"#,
    )
    .unwrap();
    assert_eq!(
        sessions(&client(home, &base), cwd).unwrap(),
        agent.join("sessions")
    );

    let overridden = client(
        home,
        &[
            base[0].clone(),
            (
                "PI_CODING_AGENT_SESSION_DIR",
                text(&home.join("env-sessions")),
            ),
        ],
    );
    assert_eq!(
        sessions(&overridden, cwd).unwrap(),
        home.join("env-sessions")
    );
    // OMP 18.18.8 keeps image blobs in its data root under a session-directory override.
    assert_eq!(blobs(&overridden, cwd).unwrap(), agent.join("blobs"));
}

#[test]
fn scans_find_flat_and_per_workspace_session_files_but_not_session_artifacts() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let workspace = root.join("--work--");
    std::fs::create_dir_all(workspace.join("2026_a/local")).unwrap();
    std::fs::write(workspace.join("2026_a.jsonl"), "").unwrap();
    // A child transcript under its parent's artifacts is not a root session.
    std::fs::write(workspace.join("2026_a/Prober.jsonl"), "").unwrap();
    // OMP 18.18.8 writes session files directly into a session-directory override.
    std::fs::create_dir_all(root.join("2026_b")).unwrap();
    std::fs::write(root.join("2026_b.jsonl"), "").unwrap();
    std::fs::write(root.join("2026_b/Child.jsonl"), "").unwrap();
    std::fs::write(root.join("notes.txt"), "").unwrap();

    let mut found = scan(root).unwrap();
    found.sort();

    assert_eq!(
        found,
        [
            root.join("--work--/2026_a.jsonl"),
            root.join("2026_b.jsonl")
        ]
    );
    assert!(scan(&root.join("missing")).unwrap().is_empty());
}

#[test]
fn oversized_session_directories_are_truncated_instead_of_failing() {
    let root = tempfile::tempdir().unwrap();
    for index in 0..4 {
        std::fs::File::create(root.path().join(format!("{index}.jsonl"))).unwrap();
    }

    let files = scan_within(root.path(), 3).unwrap();

    assert_eq!(files.len(), 3);
    assert_eq!(scan(root.path()).unwrap().len(), 4);
}
