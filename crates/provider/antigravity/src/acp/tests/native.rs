//! Identical adapter scenario against the real CLI recorder and the offline stdio peer.

use std::path::Path;

use provider_core::protocol::prompt::PromptImage;
use provider_native::replay::{Recording, StdioPeer};

use super::*;

const TEXT: &str =
    "Remember the code ACP_NATIVE_271. Reply with exactly that code. Do not use tools.";
const RECALL: &str = "Repeat the code I asked you to remember. Do not use tools.";

async fn scenario(peer: &StdioPeer) {
    let client = AntigravityClient::new(peer.program().to_owned())
        .with_image_directory(Path::new(peer.cwd()).join("images"));
    let mut spec = AgentSessionSpec {
        provider: PROVIDER.to_owned(),
        cwd: peer.cwd().to_owned(),
        config: StoredAgentConfig {
            model: Some("gemini-3.8-flash".to_owned()),
            thinking_option_id: Some("low".to_owned()),
            ..Default::default()
        },
        confinement: None,
    };
    let mut session = open(&client, &spec, None).await.unwrap();
    let handle = session.persistence().unwrap();
    for text in [TEXT, RECALL] {
        session.start_turn(text, &spec.config).await.unwrap();
        let events = finish(&mut session).await;
        assert!(
            matches!(events.last(),Some(AgentTurnEvent::Completed(Some(text))) if text.contains("ACP_NATIVE_271")),
            "{:?}",
            session.failure_message()
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, AgentTurnEvent::RequestUsage(_)))
                .count(),
            1
        );
    }
    spec.config.mode_id = Some("plan".to_owned());
    session.start_turn(RECALL, &spec.config).await.unwrap();
    assert!(
        matches!(finish(&mut session).await.last(),Some(AgentTurnEvent::Completed(Some(text))) if text.contains("ACP_NATIVE_271"))
    );
    assert_eq!(session.persistence().unwrap().session_id, handle.session_id);
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let mut session = open(&client, &spec, Some(&handle)).await.unwrap();
    let prompt = AgentPrompt { text:"This is a small test image. Reply IMAGE_RECEIVED. Do not use tools.".to_owned(),
        images:vec![PromptImage { mime_type:"image/png".to_owned(),
            data:"iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aKd0AAAAASUVORK5CYII=".to_owned() }],
        client_message_id:Some("native-image".to_owned()),..Default::default() };
    session.start_input(&prompt, &spec.config).await.unwrap();
    let events = finish(&mut session).await;
    assert!(
        matches!(events.last(), Some(AgentTurnEvent::Completed(_))),
        "{:?}",
        session.failure_message()
    );
    assert!(events.iter().any(|event|matches!(event,AgentTurnEvent::Timeline(item) if item.item["clientMessageId"] == "native-image" && item.item["imageSources"].as_array().is_some_and(|items|items.len()==1))));
    session.close().await.unwrap();
    // These opens acknowledge all native modes; only explicit Full Access gets the flag.
    for mode in ["accept-edits", "full-access"] {
        spec.config.mode_id = Some(mode.to_owned());
        let mut session = open(&client, &spec, Some(&handle)).await.unwrap();
        assert_eq!(session.runtime().mode_id.as_deref(), Some(mode));
        if mode == "full-access" {
            std::fs::write(Path::new(peer.cwd()).join("marker.txt"), "ACP_MARKER").unwrap();
            session.start_turn("Read marker.txt with the file reading tool. Create result.txt containing ACP_FILE_OK using the file writing tool. Then invoke exactly one native subagent to reply ACP_CHILD_OK without using tools. Reply ACP_TOOLS_OK after it completes.",&spec.config).await.unwrap();
            let events = finish(&mut session).await;
            assert!(
                matches!(events.last(), Some(AgentTurnEvent::Completed(_))),
                "{:?}",
                session.failure_message()
            );
            for kind in ["read", "write", "sub_agent"] {
                assert!(events.iter().any(|event|matches!(event,AgentTurnEvent::Timeline(item) if item.item["type"] == "tool_call" && item.item["detail"]["type"] == kind && item.item["status"] == "completed")),"missing {kind}");
            }
        }
        session.close().await.unwrap();
    }
}

#[tokio::test]
#[ignore = "requires authenticated native AGY; records five prompts in a temporary workspace"]
async fn record_native_acp() {
    let program = std::env::var("AIT_AGY_TEST_BIN").expect("explicit official CLI under test");
    let output = std::env::var("AIT_AGY_RECORDING").expect("private recording output path");
    let home = std::env::var("HOME").unwrap();
    let peer = StdioPeer::record(
        "agy",
        Path::new(&output),
        &Recording {
            program: Path::new(&program),
            substitutions: &[("{{home}}", &home)],
            environment: &[],
            redact: &["availableCommands", "tier"],
            meta: &json!({"provider":"antigravity","protocol":"acp","cliVersion":"1.5.18","platform":"linux-x86_64",
            "scenario":"multi-turn, mode change, close/resume, image, file tools, native subagent",
            "knownLimitations":"ACP omits native subagent conversation ID and log URI"}),
        },
    );
    scenario(&peer).await;
}

#[tokio::test]
async fn replay_native_acp() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tests/fixtures/acp-native.ndjson");
    let peer = StdioPeer::replay("agy", &path, &[("{{home}}", "/tmp/ait-native-home")]);
    scenario(&peer).await;
    peer.assert_replayed();
}
