use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use domain::summary::{SummaryError, SummaryKind, SummaryRequest, SummarySelection};
use model::summary::SummaryConfiguration;
use serde_json::{Value, json};

use super::*;

/// Live provider and model preferences, replaced whole by the test.
#[derive(Debug, Default)]
struct Configuration(Mutex<Value>);

impl SummaryConfiguration for Configuration {
    fn current(&self) -> Result<Value, SummaryError> {
        Ok(self.0.lock().unwrap().clone())
    }

    fn project(&self, _: &str) -> Value {
        Value::Null
    }
}

/// The fake native program `name` among the fixtures of the sibling crate `owner`.
fn fake(owner: &str, name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(owner)
        .join("tests/fixtures")
        .join(name)
}

#[tokio::test]
async fn configured_codex_and_claude_generate_summary_without_foreground_agents() {
    let root = tempfile::tempdir().unwrap();
    // Keep the executable immutable while concurrent tests spawn native processes; the link
    // also scopes the fake's optional diagnostic cwd overrides to this test.
    let codex = root.path().join("codex");
    symlink(fake("codex", "codex_app_server.py"), &codex).unwrap();
    let cwd = root.path().join("work");
    std::fs::create_dir(&cwd).unwrap();
    let config = Arc::new(Configuration::default());
    let providers = configured(root.path(), |name| {
        Some(match name {
            "AIT_SERVER_CODEX_BIN" => codex.clone().into_os_string(),
            "AIT_SERVER_CLAUDE_BIN" => fake("claude", "claude_code.py").into_os_string(),
            _ => root.path().join(name).into_os_string(),
        })
    });
    let generator = providers.summary_generator(config.clone());
    std::fs::write(
        cwd.join("metadata-response.json"),
        r#"{"title":"Configured metadata"}"#,
    )
    .unwrap();

    for provider in ["codex", "claude"] {
        *config.0.lock().unwrap() = json!({"providers":{
            "codex":{"enabled":provider == "codex"},
            "claude":{"enabled":provider == "claude"}
        },"metadataGeneration":{"providers":[{"provider":provider,"model":"metadata-only"}]}});
        let request = SummaryRequest {
            kind: SummaryKind::Title,
            cwd: cwd.to_str().unwrap().to_owned(),
            context: "Fix metadata assembly".into(),
            selection: Some(SummarySelection {
                provider: provider.into(),
                model: Some("metadata-only".into()),
                thinking_option_id: None,
            }),
        };
        assert_eq!(
            generator.generate(request.clone()).await.unwrap()["title"],
            "Configured metadata"
        );
        if provider == "claude" {
            generator.shutdown();
            assert_eq!(
                generator.generate(request).await,
                Err(SummaryError::Cancelled)
            );
        }
    }
    assert!(!root.path().join("agents").exists());
    let requests: Vec<Value> = std::fs::read_to_string(cwd.join("native-requests.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(requests.iter().any(|request| {
        request["method"] == "thread/start" && request["params"]["ephemeral"] == true
    }));
    let claude_args: Vec<String> =
        serde_json::from_slice(&std::fs::read(cwd.join("claude-args.json")).unwrap()).unwrap();
    assert!(
        claude_args
            .iter()
            .any(|arg| arg == "--no-session-persistence")
    );
}
