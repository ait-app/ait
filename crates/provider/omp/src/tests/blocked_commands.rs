//! Blocked slash commands: Ait answers them itself, so a native handler never changes a
//! GUI-owned setting, never runs a capability Ait has not taken over, and a running turn is
//! never interrupted to answer one.

use super::*;

fn command(text: &str) -> AgentPrompt {
    AgentPrompt::text(text)
}

#[test]
fn blocked_commands_are_answered_beside_foreground_input() {
    let (_root, client, _spec) = fixture();
    for text in [
        // Ait has a control for the setting.
        "/model opus",
        "/models opus",
        "/Advisor OFF",
        "/settings",
        "/hub",
        "/rewind",
        // Ait owns the setting with no control yet.
        "/advisor",
        "/skillful on",
        "/tree",
        // Ait has not delivered the ability.
        "/fork",
        "/worktree feature/x",
        "/retry",
        // Terminal-only.
        "/quit",
        "/Q",
        "/debug",
    ] {
        assert!(client.handles_out_of_band(text), "{text}");
    }
    // Built-ins run out of band too; OMP's own read-only commands stay prompts.
    for text in ["/compact", "/steer go on"] {
        assert!(client.handles_out_of_band(text), "{text}");
    }
    // OMP commands Ait leaves alone and unrelated slash text stay prompts.
    for text in ["/session", "/usage", "/steering", "hello"] {
        assert!(!client.handles_out_of_band(text), "{text}");
    }
}

#[tokio::test]
async fn a_blocked_command_answered_while_idle_does_not_start_a_turn() {
    let (root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    let prompt = AgentPrompt {
        client_message_id: Some("client-1".to_owned()),
        ..command("/model opus")
    };
    // Ait answers it without OMP: no turn starts and nothing reaches the transport.
    assert!(matches!(
        session.out_of_band(&prompt).await,
        Err(AgentSessionError::Rejected)
    ));

    let card = match next(session.as_mut()).await.unwrap() {
        AgentTurnEvent::Timeline(entry) => entry.item,
        other => panic!("expected the command card, got {other:?}"),
    };
    assert_eq!(card["type"], "tool_call");
    assert_eq!(card["name"], "model");
    assert_eq!(
        card["detail"]["label"],
        "`/model` is managed by Ait. Change it in the model selector."
    );

    // The other answer classes read their own way and still reach no transport call.
    for (text, name, label) in [
        (
            "/fork",
            "fork",
            "`/fork` is not available in Ait yet, so it is not sent to OMP.",
        ),
        (
            "/quit",
            "quit",
            "`/quit` is a terminal-only OMP command, which Ait does not run.",
        ),
    ] {
        assert!(matches!(
            session
                .out_of_band(&AgentPrompt {
                    client_message_id: Some(format!("client-{name}")),
                    ..command(text)
                })
                .await,
            Err(AgentSessionError::Rejected)
        ));
        let card = match next(session.as_mut()).await.unwrap() {
            AgentTurnEvent::Timeline(entry) => entry.item,
            other => panic!("expected the command card for {text}, got {other:?}"),
        };
        assert_eq!(card["type"], "tool_call", "{text}");
        assert_eq!(card["name"], name, "{text}");
        assert_eq!(card["detail"]["label"], label, "{text}");
    }
    assert_eq!(count(&calls_after(root.path(), 0), "prompt"), 0);

    // The session still runs ordinary turns afterwards.
    session.start_turn("hello", &spec.config).await.unwrap();
    assert!(matches!(
        complete(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(Some(reply))) if reply == "Hello world"
    ));
    session.close().await.unwrap();
}

#[tokio::test]
async fn a_blocked_command_running_mid_turn_is_a_card_and_leaves_the_turn_alone() {
    let (root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    let turn = session.start_turn("wait", &spec.config).await.unwrap();
    // The run is under way, so the native turn is live while the command is answered.
    next(session.as_mut()).await.unwrap();

    let prompt = AgentPrompt {
        client_message_id: Some("client-1".to_owned()),
        ..command("/advisor off")
    };
    assert!(matches!(
        session.out_of_band(&prompt).await,
        Err(AgentSessionError::Rejected)
    ));
    // The native run is still the one the command answered beside.
    session.steer_turn(&turn, "continue").await.unwrap();

    let events = complete(session.as_mut()).await;

    // The blocked command answers as a card, and the turn then completes normally instead of
    // being aborted to answer it.
    assert!(
        matches!(events.last(), Some(AgentTurnEvent::Completed(_))),
        "{events:?}"
    );
    let cards: Vec<_> = timeline_items(&events)
        .into_iter()
        .filter(|item| item["type"] == "tool_call")
        .collect();
    assert_eq!(cards.len(), 1, "{events:?}");
    let card = cards[0];
    assert_eq!(card["name"], "advisor");
    assert_eq!(card["status"], "completed");
    assert_eq!(card["error"], Value::Null);
    assert_eq!(card["metadata"]["source"], "omp_command");
    assert_eq!(card["metadata"]["nativeType"], "slash_command");
    assert_eq!(card["metadata"]["synthetic"], true);
    assert!(
        card["callId"]
            .as_str()
            .is_some_and(|id| id.starts_with("native:omp:")),
        "{card}"
    );
    assert_eq!(card["detail"]["type"], "plain_text");
    assert_eq!(
        card["detail"]["label"],
        "`/advisor` is managed by Ait, which has no control for it yet."
    );
    assert_eq!(card["detail"]["text"], card["detail"]["label"]);
    session.close().await.unwrap();

    // Nothing reached OMP: no prompt carried the command and the run was never aborted.
    let calls = calls_after(root.path(), 0);
    assert_eq!(count(&calls, "abort"), 0);
    let prompts: Vec<_> = calls
        .iter()
        .filter(|call| call["type"] == "prompt")
        .map(|call| call["message"].clone())
        .collect();
    assert_eq!(prompts, [json!("wait")]);
    // The live turn kept accepting steering, so it was never interrupted.
    assert_eq!(count(&calls, "steer"), 1);
}
