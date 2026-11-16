//! The overview shows the calling Bot conversation the notifications it has not seen yet.

use ::bot::service::Notified;
use domain::bot::notification::{Notification, NotificationKind};

use super::support::structured;
use super::*;

fn failed(id: &str, workspace: &str, conversation: domain::ConversationId) -> Notification {
    Notification {
        id: id.to_owned(),
        workspace_id: workspace.to_owned(),
        scope: Some(workspace.to_owned()),
        kind: NotificationKind::AgentFailed,
        agent_id: Some("agent-1".to_owned()),
        schedule_id: None,
        run_id: None,
        terminal_id: None,
        title: "Agent \"Fix\" failed".to_owned(),
        summary: Some("x".repeat(4096)),
        at: chrono::Utc::now(),
        conversation_id: Some(conversation),
        ticket_id: None,
        ticket_title: None,
        turn_started_at: None,
    }
}

#[tokio::test]
async fn overview_lists_unseen_notifications_of_the_calling_conversation() {
    let scoped = Scoped::start().await;
    scoped.setup(Value::Null).await;
    let conversation = scoped.conversation().await;
    let credential = scoped.bot_credential_in(conversation);
    let overview = scoped
        .call(&credential, None, "workspace_overview", json!({}))
        .await;
    assert_eq!(structured(&overview)["notifications"], json!([]));

    let admitted = scoped
        .bots
        .notify(failed("agent:agent-1:t1", &scoped.workspace, conversation))
        .await
        .unwrap();
    assert_eq!(admitted, Notified::Admitted);
    let overview = scoped
        .call(&credential, None, "workspace_overview", json!({}))
        .await;
    let listed = &structured(&overview)["notifications"];
    assert_eq!(listed.as_array().unwrap().len(), 1, "{listed}");
    assert_eq!(listed[0]["sequence"], 1);
    assert_eq!(listed[0]["kind"], "agent_failed");
    assert_eq!(listed[0]["agentId"], "agent-1");
    // The summary spills to the notification, which reads it whole (ADR-280).
    let summary = &listed[0]["summary"];
    assert_eq!(summary["preview"].as_str().unwrap().len(), 512);
    assert_eq!(summary["chars"], 1024);
    assert_eq!(summary["read"], "notification/1");
    let full = scoped
        .call(
            &credential,
            None,
            "bot_read",
            json!({"resource":summary["read"]}),
        )
        .await;
    assert_eq!(structured(&full)["summary"].as_str().unwrap().len(), 1024);

    // A session of a conversation the Bot no longer has gets no notification list.
    let stale = scoped
        .call(
            &scoped.bot_credential(),
            None,
            "workspace_overview",
            json!({}),
        )
        .await;
    assert!(structured(&stale)["notifications"].is_null());
    scoped.stop().await;
}

/// The wake state of the logged notification `id`, once it has left the inbox.
async fn settled(scoped: &Scoped, id: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let listed = scoped
                .rpc(
                    "bot.notifications.list.request",
                    json!({"workspaceId":scoped.workspace}),
                )
                .await;
            let wake = listed["notifications"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["notification"]["id"] == id)
                .map(|entry| entry["wake"].clone())
                .unwrap();
            if wake["state"] != "pending" {
                return wake;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap()
}

/// Wait until Bot turn `turn` is in `status`.
async fn turn_reaches(scoped: &Scoped, turn: &Value, status: &str) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let current = scoped
                .rpc(
                    "bot.turn.get.request",
                    json!({"workspaceId":scoped.workspace,"turnId":turn}),
                )
                .await;
            if current["turn"]["status"] == status {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn agents_a_completed_bot_turn_read_settled_do_not_wake_its_conversation_again() {
    let scoped = Scoped::start().await;
    scoped.setup(Value::Null).await;
    let conversation = scoped.conversation().await;
    let credential = scoped.bot_credential_in(conversation);
    let finished = |id: &str, agent: &str| {
        let mut notification = failed(id, &scoped.workspace, conversation);
        notification.kind = NotificationKind::AgentFinished;
        notification.agent_id = Some(agent.to_owned());
        notification.at = chrono::Utc::now() - chrono::TimeDelta::minutes(1);
        notification
    };
    scoped.turns.run();
    let sent = scoped
        .rpc(
            "bot.message.send.request",
            json!({"workspaceId":scoped.workspace,"conversationId":conversation,
                "messageId":"u1","text":"how is the fix going?"}),
        )
        .await;
    let turn = &sent["turn"]["id"];
    turn_reaches(&scoped, turn, "running").await;
    let mut agents = Vec::new();
    for tool in ["get_agent_status", "wait_for_agent", "get_agent_activity"] {
        let agent = scoped.subtree_agent().await;
        let result = scoped
            .call(&credential, None, tool, json!({"agentId":agent}))
            .await;
        assert!(succeeded(&result), "{tool}: {result}");
        agents.push((tool, agent));
    }
    let unread = scoped.subtree_agent().await;
    // Only the Bot's own sessions observe; an Agent of its subtree reading it does not.
    let subtree = scoped.subtree_credential(&unread);
    let read = scoped
        .call(
            &subtree,
            None,
            "get_agent_status",
            json!({"agentId":unread}),
        )
        .await;
    assert!(succeeded(&read), "{read}");
    for (tool, agent) in &agents {
        scoped.bots.notify(finished(tool, agent)).await.unwrap();
    }
    scoped
        .bots
        .notify(finished("unread", &unread))
        .await
        .unwrap();

    // Past a wake check: the reading turn has not replied, so nothing is settled as observed.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let listed = scoped
        .rpc(
            "bot.notifications.list.request",
            json!({"workspaceId":scoped.workspace}),
        )
        .await;
    for entry in listed["notifications"].as_array().unwrap() {
        assert_eq!(entry["wake"], json!({"state":"pending"}), "{entry}");
    }
    scoped.turns.release();
    turn_reaches(&scoped, turn, "completed").await;

    for (tool, _) in &agents {
        assert_eq!(
            settled(&scoped, tool).await,
            json!({"state":"skipped","reason":"observed"}),
            "{tool}"
        );
    }
    assert_ne!(
        settled(&scoped, "unread").await,
        json!({"state":"skipped","reason":"observed"})
    );
    scoped.stop().await;
}
