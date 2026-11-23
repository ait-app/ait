//! The adapter against transcripts recorded from the real Codex CLI; see
//! `docs/operations/provider-replay.md` for recording and refreshing them.

use std::path::PathBuf;

use provider_core::protocol::timeline::NativeItem;
use provider_native::replay::{Recording, StdioPeer};

use super::*;

const TEXT_TURN_PROMPT: &str = "Reply with the single word ok and nothing else. Do not use tools.";

/// Placeholder for the adapter's own version, which it reports in `initialize`.
const CLIENT_VERSION: &str = "{{client_version}}";

/// Placeholder for the private Codex home of the recording.
const CODEX_HOME: &str = "{{codex_home}}";

/// Variable naming the real Codex executable to record.
const RECORD_PROGRAM: &str = "AIT_CODEX_REPLAY_RECORD";

fn text_turn_transcript() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/replay/text-turn.jsonl")
}

/// What the adapter reported for one turn.
#[derive(Debug)]
struct Observed {
    timeline: Vec<NativeItem>,
    terminal: AgentTurnEvent,
}

fn text_turn_spec(cwd: &str) -> AgentSessionSpec {
    AgentSessionSpec {
        provider: "codex".to_owned(),
        cwd: cwd.to_owned(),
        config: StoredAgentConfig {
            model: Some("gpt-6-luna".to_owned()),
            mode_id: Some("read-only".to_owned()),
            thinking_option_id: Some("low".to_owned()),
            ..StoredAgentConfig::default()
        },
        confinement: None,
    }
}

/// Create a session, run one turn of `prompt` to its terminal event and close the session.
async fn text_turn(peer: &StdioPeer, prompt: &str) -> Result<Observed, AgentSessionError> {
    let client = CodexClient::new(peer.program().to_owned());
    let spec = text_turn_spec(peer.cwd());
    let mut session = client.create_session(&spec).await?;
    session.start_turn(prompt, &spec.config).await?;
    let mut timeline = Vec::new();
    let terminal = tokio::time::timeout(Duration::from_secs(120), async {
        loop {
            match session.poll_turn()? {
                Some(AgentTurnEvent::Timeline(item)) => timeline.push(item),
                Some(
                    event @ (AgentTurnEvent::Completed(_)
                    | AgentTurnEvent::Cancelled
                    | AgentTurnEvent::Failed),
                ) => return Ok::<_, AgentSessionError>(event),
                Some(_) => {}
                None => tokio::time::sleep(Duration::from_millis(10)).await,
            }
        }
    })
    .await
    .map_err(|_| AgentSessionError::Failed)??;
    session.close().await?;
    Ok(Observed { timeline, terminal })
}

fn replay_substitutions() -> [(&'static str, &'static str); 2] {
    [
        (CLIENT_VERSION, env!("CARGO_PKG_VERSION")),
        (CODEX_HOME, "/nonexistent/codex-home"),
    ]
}

#[tokio::test]
async fn recorded_text_turn_completes_with_the_native_answer_in_the_timeline() {
    let peer = StdioPeer::replay("codex", &text_turn_transcript(), &replay_substitutions());

    let observed = text_turn(&peer, TEXT_TURN_PROMPT).await;

    peer.assert_replayed();
    let observed = observed.unwrap();
    assert_eq!(
        observed.terminal,
        AgentTurnEvent::Completed(Some("ok".to_owned()))
    );
    let messages: Vec<_> = observed
        .timeline
        .iter()
        .map(|entry| (&entry.item["type"], &entry.item["text"]))
        .collect();
    assert_eq!(
        messages,
        [
            (&json!("user_message"), &json!(TEXT_TURN_PROMPT)),
            (&json!("assistant_message"), &json!("ok")),
        ]
    );
    let turn = observed.timeline[0].turn_id.as_deref().unwrap();
    for entry in &observed.timeline {
        assert_eq!(entry.turn_id.as_deref(), Some(turn));
        let message = entry.item["messageId"].as_str().unwrap();
        assert_eq!(entry.key, format!("native:{turn}:{message}"));
    }
}

#[tokio::test]
async fn replay_fails_loudly_when_the_adapter_sends_a_prompt_the_recording_did_not() {
    let peer = StdioPeer::replay("codex", &text_turn_transcript(), &replay_substitutions());

    let observed = text_turn(&peer, "A different prompt").await;

    let mismatch = peer
        .mismatch()
        .expect("the replay reports the divergent frame");
    assert_eq!(mismatch["mismatch"], "frame differs from the recording");
    assert_eq!(mismatch["actual"]["method"], "turn/start");
    assert_eq!(
        mismatch["actual"]["params"]["input"][0]["text"],
        "A different prompt"
    );
    assert!(!matches!(
        observed,
        Ok(Observed {
            terminal: AgentTurnEvent::Completed(_),
            ..
        })
    ));
}

/// Record `tests/fixtures/replay/text-turn.jsonl` from the Codex executable named by
/// `AIT_CODEX_REPLAY_RECORD`, signed in through `~/.codex/auth.json` (or `$CODEX_HOME`).
#[tokio::test]
#[ignore = "runs the real Codex CLI and makes one small paid model request"]
async fn record_text_turn_transcript() {
    let program = PathBuf::from(
        std::env::var_os(RECORD_PROGRAM)
            .unwrap_or_else(|| panic!("set {RECORD_PROGRAM} to the Codex executable to record")),
    );
    let signed_in = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".codex")))
        .expect("CODEX_HOME or HOME is set")
        .join("auth.json");
    // Only the credential is copied, so personal configuration, MCP servers and plugins stay out
    // of the recording.
    let home = tempfile::tempdir().unwrap();
    let home_path = std::fs::canonicalize(home.path()).unwrap();
    std::fs::copy(&signed_in, home_path.join("auth.json")).unwrap();
    let home_path = home_path.to_str().unwrap();
    let version = std::process::Command::new(&program)
        .arg("--version")
        .output()
        .unwrap();
    let meta = json!({
        "provider": "codex",
        "protocol": "codex app-server JSON-RPC over stdio",
        "cli": String::from_utf8(version.stdout).unwrap().trim(),
        "scenario": "one text turn, then close",
        "recorded": chrono::Utc::now().date_naive().to_string(),
    });
    let peer = StdioPeer::record(
        "codex",
        &text_turn_transcript(),
        &Recording {
            program: &program,
            substitutions: &[
                (CLIENT_VERSION, env!("CARGO_PKG_VERSION")),
                (CODEX_HOME, home_path),
            ],
            environment: &[("CODEX_HOME", home_path)],
            redact: &["installationId"],
            meta: &meta,
        },
    );

    let observed = text_turn(&peer, TEXT_TURN_PROMPT).await.unwrap();

    assert!(matches!(observed.terminal, AgentTurnEvent::Completed(_)));
}
