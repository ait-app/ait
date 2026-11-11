//! Consumer-visible command provenance, refusal and native completion behavior.

use serde_json::json;

use super::{complete, fixture};
use crate::ports::agent_session::{AgentClient, AgentSessionError, AgentTurnEvent};
use crate::protocol::commands::CompletionInput;
use crate::protocol::prompt::AgentPrompt;

fn write_catalog(root: &tempfile::TempDir, entries: &serde_json::Value) {
    std::fs::create_dir_all(root.path().join("agent")).unwrap();
    std::fs::write(
        root.path().join("agent/plugins.json"),
        serde_json::to_vec(entries).unwrap(),
    )
    .unwrap();
}

#[tokio::test]
async fn blocked_builtin_text_is_refused_without_a_native_prompt() {
    let (root, client, spec) = fixture();
    write_catalog(
        &root,
        &json!([
            {"name":"model","source":"builtin","description":"Select model"},
            {"name":"quit","source":"builtin","aliases":["q"]}
        ]),
    );
    let mut session = client.create_session(&spec).await.unwrap();
    let mut prompt = AgentPrompt::text("/model gpt-5");
    prompt.client_message_id = Some("refused-model".to_owned());
    // The refusal is a typed pre-admission fact correlated with the caller's receipt, so the
    // manager settles it without consuming unrelated provider events.
    let refusal = session.input_refusal(&prompt).expect("refused builtin");
    assert_eq!(refusal.message_id.as_deref(), Some("refused-model"));
    assert_eq!(refusal.note.item["type"], "assistant_message");
    assert!(
        refusal.note.item["text"]
            .as_str()
            .unwrap()
            .contains("builtin /model command is disabled in Ait")
    );
    assert_eq!(
        session.start_input(&prompt, &spec.config).await,
        Err(AgentSessionError::Rejected)
    );
    // Rejection never becomes a native prompt or a model turn.
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert!(!calls.contains("\"type\": \"prompt\""));
    let turn = session.start_turn("ongoing", &spec.config).await.unwrap();
    assert_eq!(
        session.steer_input(&turn, &prompt).await,
        Err(AgentSessionError::Rejected)
    );
    session.close().await.unwrap();
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert!(!calls.contains("\"type\": \"steer\""));
}

#[tokio::test]
async fn same_name_plugin_keeps_native_dispatch_and_menu_ownership() {
    let (root, client, spec) = fixture();
    write_catalog(
        &root,
        &json!([
            {"name":"model","source":"extension","description":"Project model tool",
             "directOutput":"Plugin model output"},
            {"name":"record","source":"project","description":"Project recorder",
             "directOutput":"Recorded"}
        ]),
    );
    let catalog = client.commands(&spec).await.unwrap();
    let owners: Vec<_> = catalog
        .iter()
        .filter(|entry| entry["name"] == "model")
        .collect();
    assert_eq!(owners.len(), 1);
    assert_eq!(owners[0]["source"], "extension");
    assert!(catalog.iter().any(|entry| entry["name"] == "record"));

    let mut session = client.create_session(&spec).await.unwrap();
    let mut prompt = AgentPrompt::text("/model project argument");
    prompt.client_message_id = Some("plugin-model".to_owned());
    session.start_input(&prompt, &spec.config).await.unwrap();
    let events = complete(session.as_mut()).await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentTurnEvent::Completed(_)))
    );
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    let prompts: Vec<serde_json::Value> = calls
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .filter(|call| call["type"] == "prompt")
        .collect();
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0]["message"], "/model project argument");
    session.close().await.unwrap();
}

#[tokio::test]
async fn same_name_plugins_own_host_feature_names_without_interception() {
    let (root, client, spec) = fixture();
    write_catalog(
        &root,
        &json!([
            {"name":"goal","source":"plugin","description":"Project goal",
             "directOutput":"Plugin goal output"},
            {"name":"btw","source":"skill","description":"Project btw",
             "directOutput":"Plugin btw output"},
            {"name":"vibe","source":"template","description":"Project vibe",
             "directOutput":"Plugin vibe output"}
        ]),
    );
    let mut session = client.create_session(&spec).await.unwrap();
    for name in ["goal", "btw", "vibe"] {
        let mut prompt = AgentPrompt::text(&format!("/{name} project argument"));
        prompt.client_message_id = Some(format!("plugin-{name}"));
        session.start_input(&prompt, &spec.config).await.unwrap();
        let events = complete(session.as_mut()).await;
        assert!(
            events
                .iter()
                .any(|event| matches!(event, AgentTurnEvent::Completed(_))),
            "/{name} plugin must reach native completion"
        );
    }
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    let prompts: Vec<serde_json::Value> = calls
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .filter(|call| call["type"] == "prompt")
        .collect();
    assert_eq!(
        prompts.len(),
        3,
        "each plugin dispatches to the native handler exactly once"
    );
    for (call, name) in prompts.iter().zip(["goal", "btw", "vibe"]) {
        assert_eq!(call["message"], format!("/{name} project argument"));
    }
    session.close().await.unwrap();
}

#[tokio::test]
async fn missing_provenance_keeps_native_dispatch_without_name_guessing() {
    let (root, client, spec) = fixture();
    // Old-runtime catalog entries may lack source entirely; a managed name must not be
    // assumed builtin from its name alone.
    write_catalog(
        &root,
        &json!([{"name":"model","description":"Legacy entry","directOutput":"Legacy output"}]),
    );
    let mut session = client.create_session(&spec).await.unwrap();
    let mut prompt = AgentPrompt::text("/model legacy argument");
    prompt.client_message_id = Some("legacy-model".to_owned());
    session.start_input(&prompt, &spec.config).await.unwrap();
    let events = complete(session.as_mut()).await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentTurnEvent::Completed(_)))
    );
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert!(calls.contains("\"message\": \"/model legacy argument\""));
    session.close().await.unwrap();
}

#[tokio::test]
async fn catalog_projection_carries_real_provenance_aliases_and_hints() {
    let (root, client, spec) = fixture();
    write_catalog(
        &root,
        &json!([
            {"name":"deploy","source":"template","description":"Deploy",
             "aliases":["deploy:"],"input":{"hint":"[environment]"}},
            {"name":"settings","source":"builtin"}
        ]),
    );
    let catalog = client.commands(&spec).await.unwrap();
    let deploy = catalog
        .iter()
        .find(|entry| entry["name"] == "deploy")
        .unwrap();
    assert_eq!(deploy["source"], "template");
    assert_eq!(deploy["aliases"], json!(["deploy:"]));
    assert_eq!(deploy["argumentHint"], "[environment]");
    let settings = catalog
        .iter()
        .find(|entry| entry["name"] == "settings")
        .expect("hidden builtins retain native ownership");
    assert_eq!(settings["source"], "builtin");
    assert_eq!(settings["menuVisible"], false);
    let mut session = client.create_session(&spec).await.unwrap();
    assert_eq!(session.refresh_commands().await.unwrap().unwrap(), catalog);
    session.close().await.unwrap();
}

#[tokio::test]
async fn native_catalog_updates_refresh_the_session_owner() {
    let (root, client, spec) = fixture();
    std::fs::create_dir_all(root.path().join("agent")).unwrap();
    std::fs::write(root.path().join("agent/plugins.json"), "[]").unwrap();
    std::fs::write(
        root.path().join("agent/catalog-update.json"),
        serde_json::to_vec(&json!([
            {"name":"review","source":"skill"},
            {"name":"late-plugin","source":"extension","input":{"hint":"[task]"}}
        ]))
        .unwrap(),
    )
    .unwrap();
    let mut session = client.create_session(&spec).await.unwrap();
    let initial = session.refresh_commands().await.unwrap().unwrap();
    assert!(!initial.iter().any(|entry| entry["name"] == "late-plugin"));
    let mut prompt = AgentPrompt::text("/fixture-command-update");
    prompt.client_message_id = Some("catalog-update".to_owned());
    session.start_input(&prompt, &spec.config).await.unwrap();
    let events = complete(session.as_mut()).await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentTurnEvent::Completed(_)))
    );
    let mut notified = events
        .iter()
        .any(|event| matches!(event, AgentTurnEvent::CommandsChanged));
    // The native update frame can trail the terminal result; wait until the session
    // catalog actually reflects it instead of racing the transport.
    let refreshed = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            while let Some(event) = session.poll_turn().unwrap() {
                notified |= matches!(event, AgentTurnEvent::CommandsChanged);
            }
            let catalog = session.refresh_commands().await.unwrap().unwrap();
            if notified && catalog.iter().any(|entry| entry["name"] == "late-plugin") {
                return catalog;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("catalog update must reach the live session");
    let late = refreshed
        .iter()
        .find(|entry| entry["name"] == "late-plugin")
        .expect("catalog update must reach the live session");
    assert_eq!(late["argumentHint"], "[task]");
    session.close().await.unwrap();
}

#[tokio::test]
async fn negotiated_completion_preserves_native_edits_and_filters_blocked_results() {
    let (root, client, spec) = fixture();
    write_catalog(&root, &json!([{"name":"model","source":"builtin"}]));
    std::fs::write(
        root.path().join("agent/completion.json"),
        serde_json::to_vec(&json!({
            "supported": true,
            "items": [
                {"id":"review","label":"/review","insertText":"/review ",
                 "replaceRange":{"start":0,"end":5}},
                {"id":"model","label":"/model","insertText":"/model ",
                 "replaceRange":{"start":0,"end":5}}
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    let mut session = client.create_session(&spec).await.unwrap();
    let input = CompletionInput {
        text: "/revi".to_owned(),
        cursor: 5,
    };
    let result = session.complete_commands(&input).await.unwrap();
    assert!(result.supported);
    let ids: Vec<_> = result.items.iter().map(|item| item.id.as_str()).collect();
    assert_eq!(ids, vec!["review"]);
    assert_eq!(result.items[0].insert_text, "/review ");
    session.close().await.unwrap();
}

#[tokio::test]
async fn unknown_source_claim_keeps_native_dispatch_against_a_builtin_duplicate() {
    // Two entries can claim one name; an entry without provenance must downgrade the name
    // to native dispatch whichever entry the runtime lists first.
    for builtin_first in [true, false] {
        let (root, client, spec) = fixture();
        let builtin = json!({"name":"model","source":"builtin","description":"Builtin",
            "directOutput":"Builtin output"});
        let legacy = json!({"name":"model","description":"No provenance",
            "directOutput":"Legacy output"});
        write_catalog(
            &root,
            &if builtin_first {
                json!([builtin, legacy])
            } else {
                json!([legacy, builtin])
            },
        );
        let catalog = client.commands(&spec).await.unwrap();
        let owners: Vec<_> = catalog
            .iter()
            .filter(|entry| entry["name"] == "model" && entry["menuVisible"] != false)
            .collect();
        assert_eq!(owners.len(), 1, "menu must advertise exactly one owner");
        assert!(
            owners[0]["source"].is_null(),
            "the advertised owner is the entry without provenance: {owners:?}"
        );
        // The refused builtin keeps an ownership entry so a client shortcut cannot claim it.
        assert_eq!(
            catalog
                .iter()
                .filter(|entry| entry["name"] == "model" && entry["source"] == "builtin")
                .count(),
            1
        );

        let mut session = client.create_session(&spec).await.unwrap();
        let mut prompt = AgentPrompt::text("/model project argument");
        prompt.client_message_id = Some(format!("duplicate-{builtin_first}"));
        session.start_input(&prompt, &spec.config).await.unwrap();
        let events = complete(session.as_mut()).await;
        assert!(
            events
                .iter()
                .any(|event| matches!(event, AgentTurnEvent::Completed(_))),
            "a duplicate claim never becomes a local refusal: {events:?}"
        );
        let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
        assert!(calls.contains("\"message\": \"/model project argument\""));
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn unrecognized_source_on_a_blocked_name_keeps_native_dispatch() {
    let (root, client, spec) = fixture();
    write_catalog(
        &root,
        &json!([{"name":"model","source":"mystery-kind","description":"Unknown owner",
            "directOutput":"Unknown output"}]),
    );
    let mut session = client.create_session(&spec).await.unwrap();
    let mut prompt = AgentPrompt::text("/model argument");
    prompt.client_message_id = Some("unknown-source".to_owned());
    session.start_input(&prompt, &spec.config).await.unwrap();
    complete(session.as_mut()).await;
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert!(calls.contains("\"message\": \"/model argument\""));
    session.close().await.unwrap();
}

#[tokio::test]
async fn completion_edits_are_attributed_to_the_resulting_command_source() {
    let (root, client, spec) = fixture();
    write_catalog(&root, &json!([{"name":"model","source":"builtin"}]));
    // Two editor states: a multi-byte range (native offsets are UTF-16, never byte slicing) and
    // a leading-space draft where the accepted edit is what creates the refused command.
    for (input, (start, end)) in [
        (
            CompletionInput {
                text: "/é".to_owned(),
                cursor: 2,
            },
            (0, 2),
        ),
        (
            CompletionInput {
                text: " /é".to_owned(),
                cursor: 3,
            },
            (1, 3),
        ),
    ] {
        std::fs::write(
            root.path().join("agent/completion.json"),
            serde_json::to_vec(&json!({
                "supported": true,
                "items": [
                    {"id":"model","label":"/model","insertText":"/model ",
                     "replaceRange":{"start":start,"end":end}},
                    {"id":"review","label":"/review","insertText":"/review ",
                     "replaceRange":{"start":start,"end":end}}
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        for result in [client.complete_commands(&spec, &input).await.unwrap(), {
            let mut session = client.create_session(&spec).await.unwrap();
            let result = session.complete_commands(&input).await.unwrap();
            session.close().await.unwrap();
            result
        }] {
            let ids: Vec<_> = result.items.iter().map(|item| item.id.as_str()).collect();
            assert_eq!(ids, vec!["review"], "blocked builtin must not complete");
            assert_eq!(result.items[0].insert_text, "/review ");
        }
    }
}

#[tokio::test]
async fn older_runtimes_without_completion_downgrade_without_io() {
    let (root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    let input = CompletionInput {
        text: "/revi".to_owned(),
        cursor: 5,
    };
    let result = session.complete_commands(&input).await.unwrap();
    assert!(!result.supported);
    assert_eq!(
        result.items,
        [] as [crate::protocol::commands::CompletionItem; 0]
    );
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert!(!calls.contains("\"type\": \"complete\""));
    session.close().await.unwrap();
}

#[tokio::test]
async fn draft_completion_queries_never_register_an_agent() {
    let (root, client, spec) = fixture();
    std::fs::create_dir_all(root.path().join("agent")).unwrap();
    std::fs::write(
        root.path().join("agent/completion.json"),
        serde_json::to_vec(&json!({
            "supported": true,
            "items": [{"id":"review","label":"/review","insertText":"/review ",
                "replaceRange":{"start":0,"end":5}}]
        }))
        .unwrap(),
    )
    .unwrap();
    let input = CompletionInput {
        text: "/revi".to_owned(),
        cursor: 5,
    };
    let result = client.complete_commands(&spec, &input).await.unwrap();
    assert!(result.supported);
    assert_eq!(result.items.len(), 1);
    assert!(!root.path().join("agent/sessions").exists());
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert!(!calls.contains("\"type\": \"prompt\""));
}
