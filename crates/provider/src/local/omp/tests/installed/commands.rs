//! Real installed OMP command provenance and completion facts; no model inference.

use std::fmt::Write as _;

use super::*;
use crate::ports::agent_session::{AgentClient, AgentSessionError, AgentTurnEvent};
use crate::protocol::commands::CompletionInput;

#[tokio::test]
#[ignore = "requires AIT_TEST_OMP_BIN; uses isolated configuration without model inference"]
async fn installed_omp_catalog_keeps_provenance_and_refuses_managed_builtins() {
    let root = tempfile::tempdir().unwrap();
    let program = std::env::var_os("AIT_TEST_OMP_BIN").expect("set AIT_TEST_OMP_BIN");
    let agent = root.path().join("agent");
    std::fs::create_dir_all(&agent).unwrap();
    std::fs::write(
        agent.join("models.yml"),
        json!({"providers":{"ait-local-fixture":{
        "baseUrl":"http://127.0.0.1:9/v1","api":"openai-completions","auth":"none",
        "models":[{"id":"fixture","name":"Local fixture","reasoning":false,"input":["text"],
            "contextWindow":32768,"maxTokens":1024,
            "cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0}}]}}})
        .to_string(),
    )
    .unwrap();
    let client = OmpClient {
        environment: AgentEnvironment::try_from(BTreeMap::from([
            (
                "PI_CODING_AGENT_DIR".to_owned(),
                agent.to_str().unwrap().to_owned(),
            ),
            ("OMP_PROFILE".to_owned(), String::new()),
            ("PI_PROFILE".to_owned(), String::new()),
        ]))
        .unwrap(),
        ..OmpClient::new(program.into()).with_image_directory(root.path().join("images"))
    };
    let spec = AgentSessionSpec {
        provider: "omp".to_owned(),
        cwd: root.path().to_str().unwrap().to_owned(),
        config: StoredAgentConfig::default(),
        confinement: None,
    };
    let catalog = client.commands(&spec).await.unwrap();
    // GUI-owned and terminal-only builtins never appear in the Ait menu; whichever of them
    // this runtime ships keeps an ownership entry so its name is never a free client shortcut.
    for blocked in ["model", "settings", "quit", "hotkeys", "goal", "btw"] {
        for entry in catalog.iter().filter(|entry| entry["name"] == blocked) {
            assert_eq!(
                entry["menuVisible"], false,
                "/{blocked} leaked into the menu"
            );
        }
    }
    // Every surviving entry carries its real native source.
    assert!(catalog.iter().all(|entry| entry["source"].is_string()));

    let mut session = client.create_session(&spec).await.unwrap();
    let mut prompt = AgentPrompt::text("/model fixture");
    prompt.client_message_id = Some("installed-refusal".to_owned());
    assert_eq!(
        session.start_input(&prompt, &spec.config).await,
        Err(AgentSessionError::Rejected)
    );
    // The refusal is a typed fact correlated with the caller's receipt, and it never starts
    // a turn; the manager settles it without polling unrelated provider events.
    let refusal = session.input_refusal(&prompt).unwrap();
    assert_eq!(
        refusal.message_id.as_deref(),
        Some("installed-refusal"),
        "the refusal must correlate with the caller's receipt"
    );
    assert!(
        refusal.note.item["text"]
            .as_str()
            .unwrap()
            .contains("builtin /model command is disabled in Ait")
    );

    // Completion is either negotiated with real native edits or explicitly unsupported;
    // an older runtime must downgrade without inventing candidates.
    let input = CompletionInput {
        text: "/".to_owned(),
        cursor: 1,
    };
    let result = session.complete_commands(&input).await.unwrap();
    if result.supported {
        for item in &result.items {
            assert_ne!(item.insert_text, "");
            assert!(item.replace_range.start <= item.replace_range.end);
            assert!(item.replace_range.end <= input.text.len());
        }
    } else {
        assert_eq!(
            result.items,
            [] as [crate::protocol::commands::CompletionItem; 0]
        );
    }
    session.close().await.unwrap();
}

/// One isolated official-runtime agent directory: `models.yml` never leaves loopback and each
/// extension reports itself through a native custom message, so dispatch ownership is asserted
/// from native output rather than from the host's own bookkeeping.
fn colliding_agent(root: &std::path::Path, extensions: &[(&str, &str, &str)]) -> PathBuf {
    let agent = root.join("agent");
    std::fs::create_dir_all(agent.join("extensions")).unwrap();
    std::fs::write(
        agent.join("models.yml"),
        json!({"providers":{"ait-local-fixture":{
        "baseUrl":"http://127.0.0.1:9/v1","api":"openai-completions","auth":"none",
        "models":[{"id":"fixture","name":"Local fixture","reasoning":false,"input":["text"],
            "contextWindow":32768,"maxTokens":1024,
            "cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0}}]}}})
        .to_string(),
    )
    .unwrap();
    let mut source = String::from(
        "import type { ExtensionAPI } from \"@oh-my-pi/pi-coding-agent\";\n\
         export default function (pi: ExtensionAPI) {\n",
    );
    for (name, marker, hint) in extensions {
        writeln!(
            source,
            "  pi.registerCommand(\"{name}\", {{ description: \"Isolated {name} extension\", \
             input: {{ hint: \"{hint}\" }}, handler: async () => {{ \
             await pi.sendMessage({{ customType: \"{name}\", content: \"{marker}\", \
             display: true }}); }} }});"
        )
        .unwrap();
    }
    source.push_str("}\n");
    std::fs::write(agent.join("extensions/colliding.ts"), source).unwrap();
    agent
}

#[tokio::test]
#[ignore = "requires AIT_TEST_OMP_BIN; uses isolated plugins without model inference"]
async fn installed_omp_extension_claims_keep_native_dispatch_over_blocked_names() {
    let root = tempfile::tempdir().unwrap();
    let program = std::env::var_os("AIT_TEST_OMP_BIN").expect("set AIT_TEST_OMP_BIN");
    let agent = colliding_agent(
        root.path(),
        &[
            ("goal", "GOAL-EXTENSION-OK", "<objective>"),
            ("record", "RECORD-EXTENSION-OK", "<file>"),
            ("model", "MODEL-EXTENSION-OK", "<model>"),
        ],
    );
    let client = OmpClient {
        environment: AgentEnvironment::try_from(BTreeMap::from([
            (
                "PI_CODING_AGENT_DIR".to_owned(),
                agent.to_str().unwrap().to_owned(),
            ),
            ("OMP_PROFILE".to_owned(), String::new()),
            ("PI_PROFILE".to_owned(), String::new()),
        ]))
        .unwrap(),
        ..OmpClient::new(program.into())
    };
    let spec = AgentSessionSpec {
        provider: "omp".to_owned(),
        cwd: root.path().to_str().unwrap().to_owned(),
        config: StoredAgentConfig::default(),
        confinement: None,
    };
    let catalog = client.commands(&spec).await.unwrap();
    // The installed runtime keeps extension ownership for names Ait refuses as builtins,
    // so a real plugin survives the menu projection with its own provenance and hint.
    for name in ["goal", "record"] {
        let entry = catalog
            .iter()
            .find(|entry| entry["name"] == name)
            .unwrap_or_else(|| panic!("extension /{name} lost menu ownership: {catalog:?}"));
        assert_eq!(entry["source"], "extension");
        assert!(
            entry["argumentHint"]
                .as_str()
                .is_some_and(|hint| !hint.is_empty())
        );
    }
    // A real builtin name cannot be shadowed by an extension in the official runtime: the
    // only claimant of /model is the builtin, which Ait refuses and hides from its menu while
    // keeping the ownership entry that stops a client shortcut from claiming the name.
    for entry in catalog.iter().filter(|entry| entry["name"] == "model") {
        assert_eq!(
            entry["menuVisible"], false,
            "managed builtin /model must stay out of the menu: {catalog:?}"
        );
    }

    let mut session = client.create_session(&spec).await.unwrap();
    for (name, marker) in [
        ("goal", "GOAL-EXTENSION-OK"),
        ("record", "RECORD-EXTENSION-OK"),
    ] {
        session
            .start_turn(&format!("/{name} probe"), &spec.config)
            .await
            .unwrap();
        let events = complete(session.as_mut()).await;
        assert!(
            matches!(events.last(), Some(AgentTurnEvent::Completed(_))),
            "/{name} did not settle as a completed native turn: {events:?}"
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, AgentTurnEvent::Timeline(entry)
                if entry.item["text"] == marker)),
            "the native plugin owner never ran /{name}: {events:?}"
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, AgentTurnEvent::Timeline(entry)
                if entry.item["text"]
                    .as_str()
                    .is_some_and(|text| text.contains("builtin /{name} command")))),
            "/{name} was refused although a real extension owns it: {events:?}"
        );
    }

    // The genuinely builtin-owned name still refuses before any native prompt.
    let mut prompt = AgentPrompt::text("/model fixture");
    prompt.client_message_id = Some("installed-extension-refusal".to_owned());
    assert_eq!(
        session.start_input(&prompt, &spec.config).await,
        Err(AgentSessionError::Rejected)
    );
    let refusal = session.input_refusal(&prompt).unwrap();
    assert_eq!(
        refusal.message_id.as_deref(),
        Some("installed-extension-refusal"),
        "the refusal must correlate with the caller's receipt"
    );
    assert!(
        refusal.note.item["text"]
            .as_str()
            .unwrap()
            .contains("builtin /model command is disabled in Ait")
    );
    session.close().await.unwrap();
}
