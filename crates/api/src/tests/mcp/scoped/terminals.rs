use std::sync::atomic::Ordering;

use super::*;

#[tokio::test]
async fn exited_terminal_output_is_readable_by_bot_and_subtree_credentials() {
    let scoped = Scoped::start().await;
    scoped
        .setup(json!({"groups":["observe","agents","terminals"],"modes":{"codex":["auto"]}}))
        .await;
    let bot = scoped.bot_credential();
    let child = scoped.subtree_agent().await;
    let subtree = scoped.subtree_credential(&child);
    let created = scoped.call(&bot, None, "terminal_create", json!({})).await;
    let terminal = structured(&created)["terminal"]["id"].clone();
    assert!(terminal.is_string(), "{created}");

    scoped.terminal_exited.store(true, Ordering::SeqCst);

    for credential in [&bot, &subtree] {
        let listed = scoped
            .call(credential, None, "terminal_list", json!({}))
            .await;
        assert_eq!(structured(&listed)["terminals"], json!([]));
        let captured = scoped
            .call(
                credential,
                None,
                "terminal_capture",
                json!({"terminalId":terminal,"scrollback":true}),
            )
            .await;
        assert_eq!(structured(&captured)["lines"], json!(["prompt"]));
    }

    scoped
        .rpc("terminal.kill.request", json!({"terminalId":terminal}))
        .await;
    let closed = scoped
        .call(
            &bot,
            None,
            "terminal_capture",
            json!({"terminalId":terminal}),
        )
        .await;
    assert_eq!(closed["result"]["isError"], true, "{closed}");
    scoped.stop().await;
}

#[tokio::test]
async fn exited_terminal_capture_keeps_workspace_and_group_restrictions() {
    let scoped = Scoped::start().await;
    scoped
        .setup(json!({"groups":["observe","agents","terminals"],"modes":{"codex":["auto"]}}))
        .await;
    let bot = scoped.bot_credential();
    let child = scoped.subtree_agent().await;
    let subtree = scoped.subtree_credential(&child);
    let own = scoped.terminal(&scoped.workspace, &scoped.cwd).await;
    let foreign = scoped.terminal(&scoped.other, &scoped.other_cwd).await;
    scoped.terminal_exited.store(true, Ordering::SeqCst);

    for credential in [&bot, &subtree] {
        for terminal in [foreign.as_str(), "unknown-terminal"] {
            let denied = scoped
                .call(
                    credential,
                    None,
                    "terminal_capture",
                    json!({"terminalId":terminal}),
                )
                .await;
            assert_eq!(denied["result"]["isError"], true, "{denied}");
            assert_eq!(
                denied["result"]["content"][0]["text"],
                model::ErrorCode::Forbidden.message()
            );
        }
    }
    scoped
        .permissions(json!({"groups":["observe"],"modes":{}}))
        .await;
    for credential in [&bot, &subtree] {
        rejected(
            &scoped
                .call(
                    credential,
                    None,
                    "terminal_capture",
                    json!({"terminalId":own}),
                )
                .await,
            "Unknown or unavailable tool",
        );
    }
    scoped.stop().await;
}
