#[cfg(unix)]
mod execution;

use super::*;

mod config;
mod history;
mod permissions;
#[cfg(unix)]
mod process;
mod streaming;

fn spec(cwd: &std::path::Path) -> AgentSessionSpec {
    AgentSessionSpec {
        provider: "claude".to_owned(),
        cwd: cwd.to_str().unwrap().to_owned(),
        config: StoredAgentConfig::default(),
        confinement: None,
    }
}

#[cfg(unix)]
pub(super) fn fixture() -> (tempfile::TempDir, ClaudeClient, AgentSessionSpec) {
    let root = tempfile::tempdir().unwrap();
    // Execute an immutable fixture instead of writing an executable while other tests spawn.
    // All mutable native state remains scoped to this test's cwd and configuration directory.
    let program =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude_code.py");
    let mut client = ClaudeClient::new(program);
    client.config_dir = Some(root.path().join("config"));
    client.deadline = Duration::from_secs(2);
    let spec = spec(root.path());
    (root, client, spec)
}

#[tokio::test]
async fn uninstalled_adapter_still_imports_sessions_and_hosts_bot_sessions_natively() {
    let root = tempfile::tempdir().unwrap();
    let client = ClaudeClient::new(root.path().join("missing-provider"));

    assert!(!client.is_available().await.unwrap());
    assert!(client.supports_session_import());
    assert_eq!(
        client.bot_session_support(),
        BotSessionSupport {
            instructions: InstructionDelivery::SystemPrompt,
            read_only_mode: Some("default"),
            tool_preapproval: true,
            ait_tools: true,
        }
    );
}
