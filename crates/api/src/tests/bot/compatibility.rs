use super::*;

// The required BOT_METHODS from the shipped v0.2.27 SDK, independent of today's catalog.
const LEGACY_BOT_METHODS: &[&str] = &[
    "bot.get.request",
    "bot.setup.request",
    "bot.update.request",
    "bot.pause.request",
    "bot.resume.request",
    "bot.remove.request",
    "bot.channel.list.request",
    "bot.channel.types.request",
    "bot.channel.create.request",
    "bot.channel.update.request",
    "bot.channel.delete.request",
    "bot.channel.status.request",
    "bot.conversation.list.request",
    "bot.conversation.open.request",
    "bot.conversation.reset.request",
    "bot.message.send.request",
    "bot.history.get.request",
    "bot.turn.get.request",
    "bot.turn.cancel.request",
    "bot.memory.get.request",
    "bot.memory.update.request",
];

#[tokio::test]
async fn legacy_bot_page_negotiates_without_allowing_a_per_chat_reset_of_shared_context() {
    let root = tempfile::tempdir().unwrap();
    let fixture = Fixture::with_services(Services {
        bot: Some(bots(root.path())),
        ..Services::default()
    })
    .await;
    let mut socket = fixture.socket().await;
    let mut offer = hello();
    offer["capabilities"] = json!(LEGACY_BOT_METHODS);
    send(&mut socket, offer).await;
    let info = receive(&mut socket).await;
    assert!(
        info["info"]["features"]
            .as_array()
            .unwrap()
            .contains(&json!("workspace-bots-v2"))
    );
    for method in LEGACY_BOT_METHODS {
        assert!(
            info["negotiated_capabilities"]
                .as_array()
                .unwrap()
                .contains(&json!(method)),
            "v0.2.27 hides the entire Bot page without {method}"
        );
    }

    let workspace = "wks_0123456789abcdef";
    let setup = request(
        &mut socket,
        "bot.setup.request",
        json!({"workspaceId":workspace,"execution":{"provider":"test"}}),
    )
    .await;
    assert_eq!(setup["type"], "response");
    let opened = request(
        &mut socket,
        "bot.conversation.open.request",
        json!({"workspaceId":workspace}),
    )
    .await;
    let conversation = opened["result"]["conversation"].clone();
    let reset = request(
        &mut socket,
        "bot.conversation.reset.request",
        json!({"workspaceId":workspace,"conversationId":conversation["id"]}),
    )
    .await;
    assert_eq!(reset["code"], "incompatible_version");
    let reopened = request(
        &mut socket,
        "bot.conversation.open.request",
        json!({"workspaceId":workspace}),
    )
    .await;
    assert_eq!(reopened["result"]["conversation"], conversation);
    let channels = request(
        &mut socket,
        "bot.channel.list.request",
        json!({"workspaceId":workspace}),
    )
    .await;
    assert_eq!(channels["result"]["channels"].as_array().unwrap().len(), 1);

    socket.close(None).await.unwrap();
    fixture.stop().await;
}
