//! Actual OMP command ownership and dispatch with an isolated extension.

use super::*;
use provider_core::ports::agent_session::CommandRoute;

#[tokio::test]
#[ignore = "requires AIT_TEST_OMP_BIN; uses an isolated extension without model inference"]
async fn installed_omp_preserves_extension_ownership_of_a_managed_name() {
    let fixture = installed_with_plugins().await;
    std::fs::write(
        fixture.root.path().join("agent/extensions/record-probe.ts"),
        r#"export default function (pi) {
          pi.registerCommand("record", { description: "record extension", handler: async () => {
            await pi.sendMessage({customType:"record-probe", content:"NATIVE-RECORD-OK", display:true});
          }});
        }"#,
    )
    .unwrap();
    let catalog = fixture.client.commands(&fixture.spec).await.unwrap();
    assert!(catalog.iter().any(|entry| entry["name"] == "record"
        && entry["source"] == "extension"
        && entry["menuVisible"] != false));
    assert!(catalog.iter().any(|entry| entry["name"] == "model"
        && entry["source"] == "builtin"
        && entry["menuVisible"] == false));
    let mut frames = crate::transport::subscribe_frames();
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    tokio::time::timeout(Duration::from_secs(20), async {
        while session.command_catalog().is_none() {
            session.poll_turn().unwrap();
            if session.command_catalog().is_none() {
                let _ = frames.recv().await;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(
        session.command_route("/record test"),
        Some(CommandRoute::Foreground)
    );
    assert_eq!(
        session.command_route("/model test"),
        Some(CommandRoute::OutOfBand)
    );
    session
        .start_turn("/record test", &fixture.spec.config)
        .await
        .unwrap();
    let events = tokio::time::timeout(Duration::from_secs(20), async {
        let mut events = Vec::new();
        loop {
            match session.poll_turn().unwrap() {
                Some(event) => {
                    let finished =
                        matches!(event, AgentTurnEvent::Completed(_) | AgentTurnEvent::Failed);
                    events.push(event);
                    if finished {
                        return events;
                    }
                }
                None => {
                    let _ = frames.recv().await;
                }
            }
        }
    })
    .await
    .unwrap();
    assert!(events.iter().any(|event| matches!(event, AgentTurnEvent::Timeline(item) if item.item.to_string().contains("NATIVE-RECORD-OK"))), "{events:?}");
    assert!(matches!(events.last(), Some(AgentTurnEvent::Completed(_))));
    session.close().await.unwrap();
}
