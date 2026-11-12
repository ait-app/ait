use std::sync::atomic::Ordering;
use std::time::Duration;

use model::ErrorCode;
use model::bot::Store;
use serde_json::{Value, json};

use crate::service::{Bots, Notified};
use crate::test_support::{Fixture, OTHER_WORKSPACE, WORKSPACE, setup};

#[tokio::test]
async fn provider_failures_are_durable_and_ambiguous_statuses_require_explicit_recovery() {
    for (status, expected) in [
        (json!({"status":"error"}), "failed"),
        (json!({"status":"unknown"}), "uncertain"),
        (json!({"status":"idle"}), "uncertain"),
    ] {
        let fixture = Fixture::new();
        *fixture.native.status.lock().unwrap() = Some(status);
        let bots = fixture.bots();
        let conversation = open(&bots, WORKSPACE).await;
        let turn = bots
            .execute(
                "bot.message.send.request",
                json!({"workspaceId":WORKSPACE,
            "conversationId":conversation,"messageId":"failure","text":"fixture"}),
            )
            .await
            .unwrap()["turn"]["id"]
            .clone();

        let settled = wait(&bots, WORKSPACE, &turn, expected).await;

        assert!(settled["turn"]["reply"].is_null());
        assert!(settled["turn"]["error"].as_str().is_some());
        bots.shutdown().await.unwrap();
        let recovered = fixture.bots();
        let persisted = recovered
            .execute(
                "bot.turn.get.request",
                json!({"workspaceId":WORKSPACE,"turnId":turn}),
            )
            .await
            .unwrap();
        assert_eq!(persisted["turn"]["status"], expected);
        recovered.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn distinct_provider_messages_stream_into_one_turn_and_finish_independently() {
    let fixture = Fixture::new();
    *fixture.native.status.lock().unwrap() = Some(json!({
        "status":"running",
        "messages":[["first","first partial"],["second","second partial"]]
    }));
    let bots = fixture.bots();
    let conversation = open(&bots, WORKSPACE).await;
    let turn = bots
        .execute(
            "bot.message.send.request",
            json!({"workspaceId":WORKSPACE,"conversationId":conversation,"messageId":"messages","text":"request"}),
        )
        .await
        .unwrap()["turn"]["id"]
        .clone();

    // Running is published before the first provider snapshot has been polled. A snapshot's
    // messages are persisted together, so the first visible message means all of it arrived.
    let streaming = wait_until(&bots, WORKSPACE, &turn, |value| {
        value["turn"]["status"] == "running"
            && value["turn"]["messages"]
                .as_array()
                .is_some_and(|messages| !messages.is_empty())
    })
    .await;
    assert_eq!(
        streaming["turn"]["messages"],
        json!([
            {"id":"first","text":"first partial","status":"completed"},
            {"id":"second","text":"second partial","status":"streaming"}
        ])
    );

    *fixture.native.status.lock().unwrap() = Some(json!({
        "status":"idle",
        "messages":[["first","first complete"],["second","second complete"]],
        "lastMessage":"second complete"
    }));
    let completed = wait(&bots, WORKSPACE, &turn, "completed").await;
    assert_eq!(completed["turn"]["reply"], "second complete");
    assert_eq!(
        completed["turn"]["messages"],
        json!([
            {"id":"first","text":"first complete","status":"completed"},
            {"id":"second","text":"second complete","status":"completed"}
        ])
    );
    bots.shutdown().await.unwrap();
}

#[tokio::test]
async fn failed_permission_denial_cancels_native_execution_and_fails_the_turn() {
    let fixture = Fixture::new();
    fixture.native.approval.store(true, Ordering::SeqCst);
    fixture.native.fail_denial.store(true, Ordering::SeqCst);
    let bots = fixture.bots();
    let conversation = open(&bots, WORKSPACE).await;
    let turn = bots.execute("bot.message.send.request", json!({
        "workspaceId":WORKSPACE,"conversationId":conversation,"messageId":"denial-failure","text":"request"
    })).await.unwrap()["turn"]["id"].clone();
    let failed = wait(&bots, WORKSPACE, &turn, "failed").await;
    assert_eq!(failed["turn"]["pendingPermissions"], json!([]));
    assert!(
        fixture
            .native
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|(_, action)| {
                matches!(
                    action,
                    provider::service::bot_execution::Action::Cancel { .. }
                )
            })
    );
    bots.shutdown().await.unwrap();
}

async fn open(bots: &Bots, workspace: &str) -> Value {
    bots.execute("bot.setup.request", setup(workspace))
        .await
        .unwrap();
    bots.execute(
        "bot.conversation.open.request",
        json!({"workspaceId":workspace}),
    )
    .await
    .unwrap()["conversation"]["id"]
        .clone()
}
async fn wait(bots: &Bots, workspace: &str, turn: &Value, status: &str) -> Value {
    wait_until(bots, workspace, turn, |value| {
        value["turn"]["status"] == status
    })
    .await
}

/// Poll `turn` until `ready` accepts its snapshot, failing after five seconds.
async fn wait_until(
    bots: &Bots,
    workspace: &str,
    turn: &Value,
    ready: impl Fn(&Value) -> bool,
) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let value = bots
                .execute(
                    "bot.turn.get.request",
                    json!({"workspaceId":workspace,"turnId":turn}),
                )
                .await
                .unwrap();
            if ready(&value) {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn builtin_turns_are_durable_idempotent_isolated_and_recoverable() {
    let fixture = Fixture::new();
    assert!(fixture.root.path().is_dir());
    let bots = fixture.bots();
    let conversation = open(&bots, WORKSPACE).await;
    let other = open(&bots, OTHER_WORKSPACE).await;
    assert_eq!(
        bots.execute(
            "bot.conversation.open.request",
            json!({"workspaceId":WORKSPACE})
        )
        .await
        .unwrap()["conversation"]["id"],
        conversation
    );
    let input = json!({"workspaceId":WORKSPACE,"conversationId":conversation,"messageId":"stable-1","text":"hello"});
    let accepted = bots
        .execute("bot.message.send.request", input.clone())
        .await
        .unwrap();
    let id = accepted["turn"]["id"].clone();
    let completed = wait(&bots, WORKSPACE, &id, "completed").await;
    assert_eq!(completed["turn"]["reply"], "reply:hello");
    assert_eq!(completed["turn"]["delivery"], "delivered");
    assert_eq!(
        bots.execute("bot.message.send.request", input)
            .await
            .unwrap()["turn"]["id"],
        id
    );
    assert_eq!(bots.execute("bot.message.send.request", json!({"workspaceId":WORKSPACE,"conversationId":conversation,"messageId":"stable-1","text":"changed"})).await, Err(ErrorCode::IdempotencyConflict));
    assert_eq!(
        bots.execute(
            "bot.history.get.request",
            json!({"workspaceId":OTHER_WORKSPACE,"conversationId":conversation})
        )
        .await,
        Err(ErrorCode::BotNotFound)
    );
    assert_eq!(
        bots.execute(
            "bot.turn.get.request",
            json!({"workspaceId":OTHER_WORKSPACE,"turnId":id})
        )
        .await,
        Err(ErrorCode::BotNotFound)
    );
    assert_ne!(conversation, other);
    bots.shutdown().await.unwrap();
    let restarted = fixture.bots();
    assert_eq!(
        restarted
            .execute(
                "bot.history.get.request",
                json!({"workspaceId":WORKSPACE,"conversationId":conversation})
            )
            .await
            .unwrap()["turns"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        fixture
            .native
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, action)| matches!(
                action,
                provider::service::bot_execution::Action::Send { .. }
            ))
            .count(),
        1
    );
    restarted.shutdown().await.unwrap();
    assert_eq!(
        restarted
            .execute("bot.get.request", json!({"workspaceId":WORKSPACE}))
            .await,
        Err(ErrorCode::ServerDraining)
    );
}

#[tokio::test]
async fn automatic_denial_queue_cancellation_memory_and_workspace_retirement() {
    let fixture = Fixture::new();
    fixture.native.approval.store(true, Ordering::SeqCst);
    fixture.native.hold.store(true, Ordering::SeqCst);
    let bots = fixture.bots();
    let conversation = open(&bots, WORKSPACE).await;
    let first = bots
        .execute(
            "bot.message.send.request",
            json!({"workspaceId":WORKSPACE,"conversationId":conversation,"messageId":"1","text":"first"}),
        )
        .await
        .unwrap()["turn"]["id"]
        .clone();
    wait(&bots, WORKSPACE, &first, "running").await;
    let queued = bots
        .execute(
            "bot.message.send.request",
            json!({"workspaceId":WORKSPACE,"conversationId":conversation,"messageId":"2","text":"queued"}),
        )
        .await
        .unwrap()["turn"]["id"]
        .clone();
    bots.execute(
        "bot.turn.cancel.request",
        json!({"workspaceId":WORKSPACE,"turnId":queued}),
    )
    .await
    .unwrap();
    wait(&bots, WORKSPACE, &queued, "cancelled").await;
    fixture.native.hold.store(false, Ordering::SeqCst);
    let completed = wait(&bots, WORKSPACE, &first, "completed").await;
    assert_eq!(completed["turn"]["pendingPermissions"], json!([]));
    let responses: Vec<_> = fixture
        .native
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter_map(|(_, action)| {
            if let provider::service::bot_execution::Action::Permission { response, .. } = action {
                Some(response.clone())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(responses, [json!({"behavior":"deny"})]);
    let memory = bots
        .execute("bot.memory.get.request", json!({"workspaceId":WORKSPACE}))
        .await
        .unwrap();
    let changed = bots
        .execute(
            "bot.memory.update.request",
            json!({"workspaceId":WORKSPACE,"text":"durable knowledge","expected":memory["revision"]}),
        )
        .await
        .unwrap();
    assert_ne!(changed["revision"], memory["revision"]);
    assert_eq!(
        bots.execute(
            "bot.memory.update.request",
            json!({"workspaceId":WORKSPACE,"text":"overwrite","expected":memory["revision"]})
        )
        .await,
        Err(ErrorCode::BotConflict)
    );
    bots.execute(
        "bot.conversation.reset.request",
        json!({"workspaceId":WORKSPACE,"conversationId":conversation}),
    )
    .await
    .unwrap();
    bots.retire(&[WORKSPACE.into()]).await.unwrap();
    assert_eq!(fixture.store.memory(WORKSPACE).unwrap(), "");
    assert_eq!(
        bots.execute(
            "bot.message.send.request",
            json!({"workspaceId":WORKSPACE,"conversationId":conversation,"messageId":"3","text":"stopped"})
        )
        .await,
        Err(ErrorCode::BotNotFound)
    );
    bots.shutdown().await.unwrap();
}

const PREFIX: &str = "<ait-bot-instructions>\n";

fn session(created: bool, delivery: &str) -> Value {
    json!({"created":created,"instructionDelivery":delivery})
}

/// Send `text` to the built-in conversation and return the turn once it reaches `status`.
async fn settle(bots: &Bots, conversation: &Value, text: &str, status: &str) -> Value {
    let turn = bots
        .execute(
            "bot.message.send.request",
            json!({"workspaceId":WORKSPACE,"conversationId":conversation,
                "messageId":uuid::Uuid::new_v4().to_string(),"text":text}),
        )
        .await
        .unwrap()["turn"]["id"]
        .clone();
    wait(bots, WORKSPACE, &turn, status).await["turn"].clone()
}

/// Reset `conversation` once its previous turn has finished running.
async fn reset(bots: &Bots, conversation: &Value) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match bots
                .execute(
                    "bot.conversation.reset.request",
                    json!({"workspaceId":WORKSPACE,"conversationId":conversation}),
                )
                .await
            {
                Err(ErrorCode::BotConflict) => tokio::time::sleep(Duration::from_millis(10)).await,
                result => break result.unwrap(),
            }
        }
    })
    .await
    .unwrap();
}

fn calls(fixture: &Fixture) -> Vec<provider::service::bot_execution::Action> {
    fixture
        .native
        .calls
        .lock()
        .unwrap()
        .iter()
        .map(|(_, action)| action.clone())
        .collect()
}

fn sent_texts(fixture: &Fixture) -> Vec<String> {
    calls(fixture)
        .into_iter()
        .filter_map(|action| match action {
            provider::service::bot_execution::Action::Send { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn turn_prefix_instructions_are_sent_on_every_turn() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    let conversation = open(&bots, WORKSPACE).await;
    fixture.native.creates.lock().unwrap().extend([
        session(true, "turn_prefix"),
        session(false, "turn_prefix"),
        session(false, "turn_prefix"),
        session(false, "turn_prefix"),
        session(true, "system_prompt"),
    ]);

    settle(&bots, &conversation, "first", "completed").await;
    settle(&bots, &conversation, "second", "completed").await;
    let memory = bots
        .execute("bot.memory.get.request", json!({"workspaceId":WORKSPACE}))
        .await
        .unwrap();
    bots.execute(
        "bot.memory.update.request",
        json!({"workspaceId":WORKSPACE,"text":"durable knowledge","expected":memory["revision"]}),
    )
    .await
    .unwrap();
    settle(&bots, &conversation, "third", "completed").await;
    reset(&bots, &conversation).await;
    settle(&bots, &conversation, "fourth", "completed").await;
    settle(&bots, &conversation, "fifth", "completed").await;

    let sent = sent_texts(&fixture);
    assert_eq!(sent.len(), 5);
    for (text, user) in [
        (&sent[0], "first"),
        (&sent[1], "second"),
        (&sent[2], "third"),
        (&sent[3], "fourth"),
    ] {
        assert!(text.starts_with(PREFIX), "{text}");
        assert!(
            text.ends_with(&format!("\n</ait-bot-instructions>\n\n{user}")),
            "{text}"
        );
    }
    assert!(sent[1].contains("(no saved preferences)"));
    assert!(sent[2].contains("durable knowledge"));
    assert_eq!(sent[4], "fifth");
    bots.shutdown().await.unwrap();
}

#[tokio::test]
async fn reset_required_rotates_the_conversation_and_retries_once() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    let conversation = open(&bots, WORKSPACE).await;
    let rejected =
        json!({"created":false,"instructionDelivery":"turn_prefix","resetRequired":true});
    fixture
        .native
        .creates
        .lock()
        .unwrap()
        .extend([rejected.clone(), session(true, "turn_prefix")]);

    let turn = settle(&bots, &conversation, "migrate", "completed").await;

    assert_eq!(turn["generation"], 1);
    let actions = calls(&fixture);
    let created: Vec<_> = actions
        .iter()
        .filter_map(|action| match action {
            provider::service::bot_execution::Action::Create { agent_id, .. } => Some(agent_id),
            _ => None,
        })
        .collect();
    assert_eq!(created.len(), 2);
    assert_ne!(created[0], created[1]);
    assert!(actions.iter().any(|action| matches!(
        action,
        provider::service::bot_execution::Action::Close { agent_id } if agent_id == created[0]
    )));
    assert!(actions.iter().any(|action| matches!(
        action,
        provider::service::bot_execution::Action::Send { agent_id, text, .. }
            if agent_id == created[1] && text.starts_with(PREFIX) && text.ends_with("migrate")
    )));
    let listed = bots
        .execute(
            "bot.conversation.list.request",
            json!({"workspaceId":WORKSPACE}),
        )
        .await
        .unwrap();
    assert_eq!(listed["conversations"][0]["generation"], 1);

    fixture
        .native
        .creates
        .lock()
        .unwrap()
        .extend([rejected.clone(), rejected]);
    let failed = settle(&bots, &conversation, "again", "failed").await;

    assert_eq!(
        failed["error"],
        "Bot session could not be switched to its safe mode; reset this conversation"
    );
    assert_eq!(sent_texts(&fixture).len(), 1);
    bots.shutdown().await.unwrap();
}

fn notification(id: &str, workspace: &str) -> domain::bot::notification::Notification {
    serde_json::from_value(json!({
        "id": id,
        "workspaceId": workspace,
        "scope": workspace,
        "kind": "agent_finished",
        "agentId": "agent-1",
        "title": "Fix login",
        "at": "2026-11-12T08:00:00Z"
    }))
    .unwrap()
}

#[tokio::test]
async fn a_notification_wakes_the_bot_once_and_an_unconfigured_workspace_stays_untouched() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    assert_eq!(
        bots.notify(notification("ignored", WORKSPACE))
            .await
            .unwrap(),
        Notified::Ignored
    );
    let conversation = open(&bots, WORKSPACE).await;

    assert_eq!(
        bots.notify(notification("a", WORKSPACE)).await.unwrap(),
        Notified::Admitted
    );
    assert_eq!(
        bots.notify(notification("a", WORKSPACE)).await.unwrap(),
        Notified::Duplicate
    );
    assert_eq!(
        bots.notify(notification("other", OTHER_WORKSPACE))
            .await
            .unwrap(),
        Notified::Ignored
    );
    let turn = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let history = bots
                .execute(
                    "bot.history.get.request",
                    json!({"workspaceId":WORKSPACE,"conversationId":conversation}),
                )
                .await
                .unwrap();
            if let Some(turn) = history["turns"]
                .as_array()
                .unwrap()
                .iter()
                .find(|turn| turn["status"] == "completed")
            {
                return turn.clone();
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();

    assert_eq!(turn["wake"]["notifications"][0]["id"], "a");
    assert!(
        turn["reply"]
            .as_str()
            .unwrap()
            .starts_with("reply:<ait-notifications>")
    );
    let listed = bots
        .execute(
            "bot.notifications.list.request",
            json!({"workspaceId":WORKSPACE}),
        )
        .await
        .unwrap();
    assert_eq!(listed["notifications"][0]["wake"]["turnId"], turn["id"]);
    assert_eq!(fixture.store.notification_logs().unwrap().len(), 1);
    bots.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_notification_offered_after_stop_is_kept_until_shutdown() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    open(&bots, WORKSPACE).await;

    // Draining stops the Bots before schedules settle the runs it cancelled.
    bots.stop();
    bots.offer(
        &tokio::runtime::Handle::current(),
        notification("settled-at-shutdown", WORKSPACE),
    );
    bots.shutdown().await.unwrap();

    let logs = fixture.store.notification_logs().unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].entries[0].notification.id, "settled-at-shutdown");
    assert_eq!(
        logs[0].entries[0].wake,
        domain::bot::notification::WakeStatus::Pending
    );
    assert_eq!(
        bots.notify(notification("after-shutdown", WORKSPACE)).await,
        Err(ErrorCode::ServerDraining)
    );
}

/// The Bot's notification log entries, newest first.
async fn logged(bots: &Bots) -> Vec<Value> {
    bots.execute(
        "bot.notifications.list.request",
        json!({"workspaceId":WORKSPACE}),
    )
    .await
    .unwrap()["notifications"]
        .as_array()
        .unwrap()
        .clone()
}

#[tokio::test]
async fn a_wake_turn_cut_short_by_shutdown_wakes_again_after_restart() {
    let fixture = Fixture::new();
    fixture.native.hold.store(true, Ordering::SeqCst);
    let bots = fixture.bots();
    open(&bots, WORKSPACE).await;
    bots.notify(notification("a", WORKSPACE)).await.unwrap();
    let first = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let entry = logged(&bots).await.remove(0);
            if entry["wake"]["state"] == "woke" {
                return entry["wake"]["turnId"].clone();
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    wait(&bots, WORKSPACE, &first, "running").await;

    bots.shutdown().await.unwrap();
    fixture.native.hold.store(false, Ordering::SeqCst);
    let restarted = fixture.bots();

    let retried = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let entry = logged(&restarted).await.remove(0);
            if entry["wake"]["state"] == "woke" && entry["wake"]["turnId"] != first {
                return entry["wake"]["turnId"].clone();
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    wait(&restarted, WORKSPACE, &retried, "completed").await;
    let cancelled = restarted
        .execute(
            "bot.turn.get.request",
            json!({"workspaceId":WORKSPACE,"turnId":first}),
        )
        .await
        .unwrap();
    assert_eq!(cancelled["turn"]["status"], "cancelled");
    restarted.shutdown().await.unwrap();
}

#[tokio::test]
async fn ticket_entry_points_open_assign_report_and_wake_the_tickets_conversation() {
    use domain::bot::ticket::{TicketDraft, TicketStatus, TicketUpdate, WorkerReportKind};

    use crate::tickets::{AssignTicket, OpenTicket, WorkerReport};

    let fixture = Fixture::new();
    let bots = fixture.bots();
    let conversation = serde_json::from_value(open(&bots, WORKSPACE).await).unwrap();
    let request = OpenTicket {
        root: WORKSPACE.into(),
        draft: TicketDraft {
            conversation_id: Some(conversation),
            key: "create-1".into(),
            title: "Fix login".into(),
            request: "The login page crashes".into(),
        },
    };

    let opened = bots.open_ticket(request.clone()).await.unwrap();
    let retried = bots.open_ticket(request).await.unwrap();
    bots.assign_ticket(AssignTicket {
        root: WORKSPACE.into(),
        ticket_id: opened.ticket_id,
        agent_id: "agent-1".into(),
        workspace_id: WORKSPACE.into(),
    })
    .await
    .unwrap();
    let status = bots
        .report_progress(WorkerReport {
            root: WORKSPACE.into(),
            ticket_id: opened.ticket_id,
            agent_id: "agent-1".into(),
            workspace_id: WORKSPACE.into(),
            kind: WorkerReportKind::Done,
            text: "Merged #12".into(),
        })
        .await
        .unwrap();
    let woke = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let entry = logged(&bots).await.remove(0);
            if entry["wake"]["state"] == "woke" {
                return entry;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    let closed = bots
        .update_ticket(
            WORKSPACE.into(),
            opened.ticket_id,
            TicketUpdate {
                status: Some(TicketStatus::Done),
                outcome: Some("Merged #12".into()),
                ..TicketUpdate::default()
            },
        )
        .await
        .unwrap();
    let abandoned = bots
        .abandon_ticket(WORKSPACE.into(), opened.ticket_id)
        .await;

    assert!(opened.created && !retried.created);
    assert_eq!(retried.ticket_id, opened.ticket_id);
    assert_eq!(status, TicketStatus::Open);
    assert_eq!(woke["wake"]["conversationId"], json!(conversation));
    assert_eq!(woke["notification"]["kind"], "ticket_done");
    assert_eq!(woke["notification"]["ticketTitle"], "Fix login");
    assert_eq!(closed["ticket"]["status"], "done");
    assert_eq!(abandoned, Ok(()));
    let listed = bots
        .execute(
            "bot.ticket.list.request",
            json!({"workspaceId":WORKSPACE,"includeClosed":true}),
        )
        .await
        .unwrap();
    assert_eq!(listed["tickets"][0]["id"], json!(opened.ticket_id));
    let reopened = bots
        .reopen_ticket(WORKSPACE.into(), opened.ticket_id, "agent-1".into())
        .await;
    let reclosed = bots
        .close_finished_ticket(WORKSPACE.into(), opened.ticket_id)
        .await;
    assert_eq!(reopened, Ok(true));
    // The `done` came before the reopen, so it says nothing about the new work.
    assert_eq!(reclosed, Ok(Some(TicketStatus::Cancelled)));
    bots.shutdown().await.unwrap();
    assert_eq!(
        bots.abandon_ticket(WORKSPACE.into(), opened.ticket_id)
            .await,
        Err(ErrorCode::ServerDraining)
    );
}
