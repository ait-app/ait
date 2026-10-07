use super::*;
use crate::ports::agent_session::AgentClient;

fn file(path: &Path, contents: &str, runnable: bool) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
    #[cfg(unix)]
    if runnable {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let _ = runnable;
}

fn bundle(root: &Path, mac: bool) -> (PathBuf, PathBuf) {
    let resources = root.join(if mac { "Resources" } else { "resources" });
    file(
        &resources.join("app/package.json"),
        r#"{"name":"@deepseek-ai/dsh-desktop"}"#,
        false,
    );
    let entrypoint = resources.join("app/dsh/node_modules/@deepseek-ai/dsh/lib/bin.js");
    file(&entrypoint, "// bundled CLI", false);
    let program = if mac {
        root.join("MacOS/DeepSeek Harness")
    } else {
        resources.join("runtime/primary-runtime/dependencies/node/bin/node")
    };
    file(&program, "#!/bin/sh\nexit 0\n", true);
    (program, entrypoint)
}

#[tokio::test]
async fn desktop_runtime_is_available_without_a_path_cli() {
    let temp = tempfile::tempdir().unwrap();
    let (program, entrypoint) = bundle(temp.path(), false);
    let client = resolve(&[], &[temp.path().into()]);
    assert_eq!(client.program, program);
    assert!(client.is_available().await.unwrap());
    let mut command = client.command();
    command.arg("--version");
    assert_eq!(
        command.as_std().get_args().collect::<Vec<_>>(),
        vec![entrypoint.as_os_str(), std::ffi::OsStr::new("--version")]
    );
    std::fs::remove_file(entrypoint).unwrap();
    assert!(!client.is_available().await.unwrap());
}

#[test]
fn path_cli_precedes_desktop_and_explicit_programs_remain_unchanged() {
    let temp = tempfile::tempdir().unwrap();
    bundle(temp.path(), false);
    let bin = temp.path().join("bin");
    let cli = bin.join(if cfg!(windows) { "dsh.exe" } else { "dsh" });
    file(&cli, "#!/bin/sh\nexit 0\n", true);
    let client = resolve(&[bin], &[temp.path().into()]);
    assert_eq!(client.program, cli);
    assert!(client.desktop.is_none());
    let explicit = DeepSeekHarnessClient::new("custom-dsh".into());
    assert_eq!(explicit.command().as_std().get_program(), "custom-dsh");
    assert_eq!(explicit.command().as_std().get_args().count(), 0);
}

#[test]
fn incomplete_or_unrelated_desktop_bundles_are_skipped() {
    let temp = tempfile::tempdir().unwrap();
    assert!(desktop(temp.path()).is_none());
    let (node, _) = bundle(temp.path(), false);
    std::fs::remove_file(node).unwrap();
    assert!(desktop(temp.path()).is_none());
    bundle(temp.path(), false);
    file(
        &temp.path().join("resources/app/package.json"),
        r#"{"name":"other-app"}"#,
        false,
    );
    assert!(desktop(temp.path()).is_none());
    assert_eq!(
        resolve(&[], &[temp.path().into()]).program,
        Path::new("dsh")
    );
}

#[test]
fn mac_bundle_uses_electron_as_node_and_preserves_paths_with_spaces() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("DeepSeek Harness.app/Contents");
    let (program, entrypoint) = bundle(&root, true);
    let mut bundles = Vec::new();
    applications(temp.path(), &mut bundles);
    let client = resolve(&[], &bundles);
    assert_eq!(client.program, program);
    let command = client.command();
    assert_eq!(
        command.as_std().get_args().next(),
        Some(entrypoint.as_os_str())
    );
    assert!(
        command
            .as_std()
            .get_envs()
            .any(|(key, value)| key == "ELECTRON_RUN_AS_NODE"
                && value == Some(std::ffi::OsStr::new("1")))
    );
    file(
        &root.join("MacOS/another-binary"),
        "#!/bin/sh\nexit 0\n",
        true,
    );
    assert!(desktop(&root).is_none());
}

#[tokio::test]
#[ignore = "requires an installed DSH CLI or desktop bundle; does not open a session"]
async fn installed_launcher_runs_native_cli_without_a_wrapper() {
    let client = DeepSeekHarnessClient::installed();
    assert!(client.is_available().await.unwrap());
    let output = client.command().arg("--version").output().await.unwrap();
    assert!(output.status.success());
    assert!(!output.stdout.is_empty());
}
