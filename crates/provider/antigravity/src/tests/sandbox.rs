use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::json;

use super::*;
use crate::transport::Transport;

fn confinement(root: &Path) -> Option<domain::workspace::sandbox::WriteConfinement> {
    let workspace = serde_json::from_value(json!({
        "workspaceId": "wks_0123456789abcdef",
        "projectId": "prj_1",
        "kind": "worktree",
        "cwd": root,
        "worktreeRoot": root,
        "displayName": "sandbox",
        "createdAt": "2026-01-01T00:00:00Z",
        "updatedAt": "2026-01-01T00:00:00Z",
        "archivedAt": null,
        "sandbox": "worktree_writes"
    }))
    .unwrap();
    domain::workspace::sandbox::WriteConfinement::for_workspace(&workspace)
}

fn sandboxed_fixture() -> (TempDir, AntigravityClient, AgentSessionSpec) {
    // Place the fixture outside `/tmp`, which bubblewrap mounts writable for scratch files.
    let directory = tempfile::Builder::new()
        .prefix(".agy-sandbox-test-")
        .tempdir_in(std::env::current_dir().unwrap())
        .unwrap();
    let cwd = directory.path().join("wt");
    let home = directory.path().join("home");
    std::fs::create_dir_all(cwd.join(".git")).unwrap();
    std::fs::create_dir_all(home.join(".gemini/config")).unwrap();
    std::fs::write(
        home.join(".gemini/config/hooks.json"),
        "{\"shared\":true}\n",
    )
    .unwrap();
    let cwd = cwd.canonicalize().unwrap();
    let home = home.canonicalize().unwrap();
    let program = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tests/fixtures/agy.py");
    let mut client = AntigravityClient::new(program).with_image_directory(cwd.join("images"));
    client.environment = AgentEnvironment::try_from(BTreeMap::from([
        (
            "AGY_FIXTURE_LOG".to_owned(),
            cwd.join("requests.jsonl").to_str().unwrap().to_owned(),
        ),
        ("HOME".to_owned(), home.to_str().unwrap().to_owned()),
    ]))
    .unwrap();
    let spec = AgentSessionSpec {
        provider: PROVIDER.to_owned(),
        cwd: cwd.to_str().unwrap().to_owned(),
        config: StoredAgentConfig::default(),
        confinement: confinement(&cwd),
    };
    (directory, client, spec)
}

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

#[test]
fn confined_launch_mounts_cli_state_and_protects_configuration() {
    let root = tempfile::tempdir().unwrap();
    let worktree = root.path().join("wt");
    let home = root.path().join("home");
    std::fs::create_dir_all(worktree.join(".git")).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    let home = home.canonicalize().unwrap();
    let lookup = |name: &str| (name == "HOME").then(|| home.to_str().unwrap().to_owned());

    let command = provider_native::sandbox::command(
        Path::new("agy"),
        confinement(&worktree).as_ref(),
        provider_native::sandbox::ANTIGRAVITY_STATE,
        &lookup,
    )
    .unwrap();
    let arguments: Vec<String> = command
        .as_std()
        .get_args()
        .map(|arg| arg.to_str().unwrap().to_owned())
        .collect();
    let (trees, protected) = rules(&arguments);

    let cli = home.join(".gemini/antigravity-cli");
    assert!(trees.contains(&cli), "{trees:?}");
    assert!(!trees.contains(&home.join(".gemini/config")));
    for entry in ["builtin", "hooks.json", "plugins", "settings.json"] {
        assert!(protected.contains(&cli.join(entry)), "{entry}");
    }
    for state in ["conversations", "cache", "log", "jetski_state.pbtxt"] {
        let path = cli.join(state);
        assert!(
            !protected.iter().any(|entry| path.starts_with(entry)),
            "{state}"
        );
    }
}

#[tokio::test]
async fn confined_transport_enforces_worktree_and_state_write_boundaries() {
    let (_directory, client, spec) = sandboxed_fixture();
    let probe = r#"import json, os, pathlib, sys
home = pathlib.Path(os.environ["HOME"])
cli = home / ".gemini" / "antigravity-cli"
(cli / "conversations").mkdir(parents=True, exist_ok=True)
(cli / "conversations" / "state.pb").write_text("ok")
pathlib.Path("worktree-ok.txt").write_text("ok")
for blocked in [
    home / "forbidden.txt",
    home / ".gemini" / "config" / "hooks.json",
    cli / "settings.json",
    cli / "hooks.json",
    cli / "builtin" / "injected.md",
    cli / "plugins" / "injected.json",
]:
    try:
        blocked.write_text("tampered")
        sys.exit(20)
    except OSError:
        pass
print(json.dumps({"event": "probe_ok"}), flush=True)
"#;
    let probe_client = AntigravityClient {
        program: PathBuf::from("python3"),
        ..client
    };
    let mut transport = Transport::spawn(
        &probe_client,
        &spec.cwd,
        &["-c".to_owned(), probe.to_owned()],
        spec.confinement.as_ref(),
    )
    .unwrap();
    assert_eq!(transport.receive().await.unwrap()["event"], "probe_ok");
    transport.close().await.unwrap();
    assert_eq!(
        std::fs::read_to_string(Path::new(&spec.cwd).join("worktree-ok.txt")).unwrap(),
        "ok"
    );
}

#[tokio::test]
async fn confined_session_cancels_and_keeps_confinement_on_reconnect() {
    let (_directory, client, mut spec) = sandboxed_fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    let handle = session.persistence().unwrap();
    session.start_turn("first", &spec.config).await.unwrap();
    completed(session.as_mut()).await;

    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    session.cancel_turn(&turn).await.unwrap();
    loop {
        if event(session.as_mut()).await.unwrap() == AgentTurnEvent::Cancelled {
            break;
        }
    }

    spec.config.mode_id = Some("accept-edits".to_owned());
    session
        .start_turn("after cancel", &spec.config)
        .await
        .unwrap();
    completed(session.as_mut()).await;
    session.close().await.unwrap();

    let mut resumed = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    resumed.start_turn("resumed", &spec.config).await.unwrap();
    completed(resumed.as_mut()).await;
    resumed.close().await.unwrap();

    let session_launches: Vec<_> =
        std::fs::read_to_string(Path::new(&spec.cwd).join("requests.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .filter(|entry| entry.get("args").is_some() && entry["args"] != json!(["models"]))
            .collect();
    assert_eq!(session_launches.len(), 3);
    for launch in &session_launches {
        let args = launch["args"].as_array().unwrap();
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--settings", "{\"enableTerminalSandbox\":false}"]),
            "{args:?}"
        );
    }
}
