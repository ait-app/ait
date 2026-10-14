use serde_json::json;

use super::transport::{connect, request};
use super::{ready, start_with_path, terminate};

#[tokio::test]
async fn daemon_bot_uses_private_native_execution_and_keeps_conversation_after_reconnect() {
    let fixture = super::native::NativeFixture::new();
    let directory = fixture.root.path().join("state");
    let log = fixture.root.path().join("server.log");
    let mut process = start_with_path(&directory, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut methods: Vec<_> = bot::capabilities::implemented_methods()
        .map(|method| method.name)
        .collect();
    methods.extend(["workspace.open.request", "agent.list.request"]);
    let mut socket = connect(&address, &methods).await;
    let workspace = request(
        &mut socket,
        "workspace.open.request",
        json!({"cwd":fixture.cwd}),
    )
    .await;
    let bot_id = domain::BotId::generate();
    let created = request(
        &mut socket,
        "bot.create.request",
        json!({"botId":bot_id,
        "workspaceId":workspace["result"]["workspace"]["id"],"name":"fixture bot",
        "execution":{"provider":"codex"}}),
    )
    .await;
    assert_eq!(created["type"], "response", "{created}");
    let opened = request(
        &mut socket,
        "bot.conversation.open.request",
        json!({"botId":bot_id}),
    )
    .await;
    let conversation = opened["result"]["conversation"]["id"].clone();
    let sent = request(
        &mut socket,
        "bot.message.send.request",
        json!({"botId":bot_id,
        "conversationId":conversation,"messageId":"fixture-message","text":"bot hello"}),
    )
    .await;
    assert_eq!(sent["result"]["accepted"], true, "{sent}");
    drop(socket);
    let mut restored = connect(&address, &methods).await;
    let completed = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let turn = request(
                &mut restored,
                "bot.turn.get.request",
                json!({"botId":bot_id,
                "turnId":sent["result"]["turn"]["id"]}),
            )
            .await;
            let status = turn["result"]["turn"]["status"].as_str().unwrap();
            assert!(!matches!(status, "failed" | "uncertain"), "{turn}");
            if status == "completed" {
                break turn;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(completed["result"]["turn"]["reply"], "Echo: bot hello");
    let history = request(
        &mut restored,
        "bot.history.get.request",
        json!({"botId":bot_id,
        "conversationId":conversation}),
    )
    .await;
    assert_eq!(history["result"]["turns"].as_array().unwrap().len(), 1);
    assert!(!history.to_string().contains("sessionId"));
    let agents = request(&mut restored, "agent.list.request", json!({})).await;
    assert_eq!(agents["result"]["entries"], json!([]));
    drop(restored);
    terminate(&mut process).await;
}
