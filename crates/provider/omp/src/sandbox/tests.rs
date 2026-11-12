use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::*;
use provider_core::ports::environment::AgentEnvironment;

/// A client whose environment is exactly `variables`, so no daemon value leaks in.
fn client(variables: &[(&str, &str)]) -> OmpClient {
    let mut values = BTreeMap::from(
        [
            "OMP_PROFILE",
            "PI_PROFILE",
            "PI_CONFIG_DIR",
            "PI_CODING_AGENT_DIR",
        ]
        .map(|name| (name.to_owned(), String::new())),
    );
    values.extend(
        variables
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned())),
    );
    OmpClient {
        environment: AgentEnvironment::try_from(values).unwrap(),
        ..OmpClient::new(PathBuf::from("omp"))
    }
}

#[test]
fn the_default_agent_directory_protects_the_same_entries_as_an_overriding_one() {
    let mut nested: Vec<_> = AGENT_CONFIGURATION
        .iter()
        .map(|entry| format!("agent/{entry}"))
        .collect();
    nested.extend([".env", "natives", "plugins", "python-env"].map(str::to_owned));
    nested.sort_unstable();

    assert_eq!(ROOT_CONFIGURATION, nested);
}

#[test]
fn only_the_default_profile_keeps_every_profile_read_only() {
    let named = client(&[("HOME", "/home/u"), ("OMP_PROFILE", "work")]);
    assert!(std::ptr::eq(state(&named), PROFILE_STATE));
    // An empty OMP_PROFILE selects the default profile over PI_PROFILE, as in OMP.
    for profile in [("OMP_PROFILE", "default"), ("PI_PROFILE", "work")] {
        let default = client(&[("HOME", "/home/u"), profile]);
        assert!(std::ptr::eq(state(&default), DEFAULT_STATE), "{profile:?}");
    }
}

/// Writable trees and protected paths of a confined launch for `variables`.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn launch(worktree: &Path, variables: &[(&str, &str)]) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let client = client(variables);
    let lookup = |name: &str| {
        variables
            .iter()
            .find_map(|(key, value)| (*key == name).then(|| (*value).to_owned()))
    };
    let command = provider_native::sandbox::command(
        Path::new("omp"),
        crate::tests::confinement(worktree).as_ref(),
        state(&client),
        &lookup,
    )
    .unwrap();
    let arguments: Vec<String> = command
        .as_std()
        .get_args()
        .map(|argument| argument.to_str().unwrap().to_owned())
        .collect();
    rules(&arguments)
}

/// Seatbelt lists writable subpaths in its allow rule and protected ones in its last deny.
#[cfg(target_os = "macos")]
fn rules(arguments: &[String]) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let paths = |rule: &str| -> Vec<PathBuf> {
        rule.lines()
            .filter_map(|line| line.trim().strip_prefix("(subpath \""))
            .filter_map(|line| line.strip_suffix("\")").or(line.strip_suffix("\"))")))
            .map(PathBuf::from)
            .collect()
    };
    let profile = &arguments[1];
    let (allowed, protected) = profile.rsplit_once("(deny file-write*").unwrap();
    (paths(allowed), paths(protected))
}

/// Bubblewrap binds writable trees, then binds protected paths read-only over them.
#[cfg(target_os = "linux")]
fn rules(arguments: &[String]) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut trees = Vec::new();
    let mut protected = Vec::new();
    for window in arguments.windows(2) {
        match window[0].as_str() {
            "--bind" => trees.push(PathBuf::from(&window[1])),
            "--ro-bind" if window[1] != "/" => protected.push(PathBuf::from(&window[1])),
            _ => {}
        }
    }
    (trees, protected)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn a_confined_launch_protects_omp_configuration_in_every_root() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("wt/.git")).unwrap();
    let home = root.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let home = home.canonicalize().unwrap();
    let data = home.join("data");
    let variables = [
        ("HOME", home.to_str().unwrap()),
        ("XDG_DATA_HOME", data.to_str().unwrap()),
    ];

    let (trees, protected) = launch(&root.path().join("wt"), &variables);

    for tree in [".omp", ".omp/agent", "data/omp"] {
        assert!(trees.contains(&home.join(tree)), "{tree}: {trees:?}");
    }
    for entry in [
        ".omp/.env",
        ".omp/agent/config.yml",
        ".omp/agent/models.yml",
        ".omp/agent/extensions",
        ".omp/agent/settings.json",
        ".omp/natives",
        ".omp/plugins",
        ".omp/profiles",
        "data/omp/natives",
        "data/omp/plugins",
        "data/omp/profiles",
    ] {
        assert!(protected.contains(&home.join(entry)), "{entry}");
    }
    for state in ["sessions", "blobs", "agent.db"] {
        let path = home.join(".omp/agent").join(state);
        assert!(
            !protected.iter().any(|entry| path.starts_with(entry)),
            "{state}"
        );
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn overrides_and_named_profiles_move_the_protected_configuration() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("wt/.git")).unwrap();
    let home = root.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let home = home.canonicalize().unwrap();
    let agent = home.join("custom-agent");
    let variables = [
        ("HOME", home.to_str().unwrap()),
        ("PI_CONFIG_DIR", ".omp-alt"),
        ("PI_CODING_AGENT_DIR", agent.to_str().unwrap()),
        ("OMP_PROFILE", "work"),
    ];

    let (trees, protected) = launch(&root.path().join("wt"), &variables);

    assert!(trees.contains(&agent));
    assert!(trees.contains(&home.join(".omp-alt")));
    for entry in [
        "config.yml",
        "config.yaml",
        "mcp.json",
        ".env",
        "hooks",
        "tools",
    ] {
        assert!(protected.contains(&agent.join(entry)), "{entry}");
        let default = home.join(".omp-alt/agent").join(entry);
        assert!(protected.contains(&default), "{entry}");
    }
    // A named profile writes below `profiles/<name>`.
    assert!(!protected.iter().any(|path| path.ends_with("profiles")));
}
