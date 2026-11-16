//! Bot tickets on the scoped MCP (ADR-274): the ticket every Bot-started Agent joins, the
//! Bot's ticket tools, the worker's progress report and the overview's open tickets.

use domain::bot::{BOT_LABEL_PREFIX, BOT_TICKET_LABEL};
use domain::{ConversationId, TicketId};

use super::support::{codex_modes, forbidden, rejected, structured, succeeded};
use super::*;

fn create(title: &str) -> Value {
    json!({"provider":"codex/offline-model","title":title,"initialPrompt":"work on it"})
}

fn text(response: &Value) -> &str {
    assert_eq!(response["result"]["isError"], true, "{response}");
    response["result"]["content"][0]["text"].as_str().unwrap()
}

impl Scoped {
    /// A Bot with Codex Agents allowed, and a session credential of one of its conversations.
    async fn ticketed() -> (Self, ConversationId, String) {
        let scoped = Self::start().await;
        scoped.setup(codex_modes(&["auto"])).await;
        let conversation = scoped.conversation().await;
        let credential = scoped.bot_credential_in(conversation);
        (scoped, conversation, credential)
    }

    /// Create an Agent through MCP as `bearer`, returning its id and the reported ticket.
    async fn started(&self, bearer: &str, arguments: Value) -> (String, Value) {
        let created = self.call(bearer, None, "create_agent", arguments).await;
        let result = structured(&created);
        let agent = result["agent"]["id"].as_str().unwrap().to_owned();
        (agent, result["ticketId"].clone())
    }

    async fn ticket(&self, ticket: &Value) -> Value {
        self.rpc(
            "bot.ticket.get.request",
            json!({"workspaceId":self.workspace,"ticketId":ticket}),
        )
        .await["ticket"]
            .clone()
    }

    async fn ticket_count(&self) -> usize {
        self.rpc(
            "bot.ticket.list.request",
            json!({"workspaceId":self.workspace,"includeClosed":true}),
        )
        .await["tickets"]
            .as_array()
            .unwrap()
            .len()
    }

    async fn label(&self, agent: &str) -> Value {
        self.rpc("agent.get.request", json!({"agentId":agent}))
            .await["agent"]["labels"][BOT_TICKET_LABEL]
            .clone()
    }

    /// Kinds of the logged notifications, oldest first.
    async fn notification_kinds(&self) -> Vec<String> {
        let listed = self
            .rpc(
                "bot.notifications.list.request",
                json!({"workspaceId":self.workspace}),
            )
            .await;
        let mut kinds: Vec<String> = listed["notifications"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["notification"]["kind"].as_str().unwrap().to_owned())
            .collect();
        kinds.reverse();
        kinds
    }
}

fn agent_ids(ticket: &Value) -> Vec<&str> {
    ticket["assignments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|assignment| assignment["agentId"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn bot_started_agent_opens_one_ticket_per_creation_key() {
    let (scoped, conversation, bot) = Scoped::ticketed().await;
    let mut keyed = create("Fix login");
    keyed["idempotencyKey"] = json!("fix-login-1");
    let (agent, ticket) = scoped.started(&bot, keyed.clone()).await;
    assert!(ticket.is_string(), "{ticket}");
    assert_eq!(scoped.label(&agent).await, ticket);
    let opened = scoped.ticket(&ticket).await;
    assert_eq!(opened["title"], "Fix login");
    assert_eq!(opened["request"], "work on it");
    assert_eq!(opened["status"], "open");
    assert_eq!(opened["conversationId"], conversation.to_string());
    assert_eq!(agent_ids(&opened), [agent.as_str()]);
    assert_eq!(opened["assignments"][0]["workspaceId"], scoped.workspace);

    // A retried creation finds the same Agent and the same ticket.
    let (retried, again) = scoped.started(&bot, keyed.clone()).await;
    assert_eq!((retried.as_str(), &again), (agent.as_str(), &ticket));
    assert_eq!(scoped.ticket_count().await, 1);
    // The same key for another task is refused before anything is created.
    keyed["title"] = json!("Something else");
    let conflict = scoped.call(&bot, None, "create_agent", keyed).await;
    assert_eq!(
        text(&conflict),
        model::ErrorCode::IdempotencyConflict.message()
    );
    // Without a key every creation is its own task.
    let (second, other) = scoped.started(&bot, create("Write docs")).await;
    assert_ne!(other, ticket);
    assert_eq!(agent_ids(&scoped.ticket(&other).await), [second.as_str()]);
    assert_eq!(scoped.ticket_count().await, 2);

    // A session whose conversation the Bot no longer has still opens a ticket, without origin.
    let (_, orphan) = scoped
        .started(&scoped.bot_credential(), create("Orphan"))
        .await;
    assert!(scoped.ticket(&orphan).await["conversationId"].is_null());
    scoped.stop().await;
}

#[tokio::test]
async fn failed_creation_leaves_no_ticket_behind() {
    let (scoped, _, bot) = Scoped::ticketed().await;
    // The host used the key for another creation, so the Agent creation conflicts after the
    // ticket was opened.
    let mut taken = json!({"workspaceId":scoped.workspace,"idempotencyKey":"taken",
        "config":{"provider":"codex","model":"offline-model","cwd":scoped.cwd,
            "title":"Host Agent"}});
    scoped.rpc("agent.create.request", taken.take()).await;
    let mut keyed = create("Conflicting");
    keyed["idempotencyKey"] = json!("taken");
    let failed = scoped.call(&bot, None, "create_agent", keyed).await;
    assert_eq!(
        text(&failed),
        model::ErrorCode::IdempotencyConflict.message()
    );
    assert_eq!(scoped.ticket_count().await, 0);
    scoped.stop().await;
}

#[tokio::test]
async fn named_ticket_takes_another_agent_unless_closed_or_foreign() {
    let (scoped, _, bot) = Scoped::ticketed().await;
    let (first, ticket) = scoped.started(&bot, create("Fix login")).await;
    let mut joining = create("Review the fix");
    joining["ticketId"] = ticket.clone();
    let (second, joined) = scoped.started(&bot, joining.clone()).await;
    assert_eq!(joined, ticket);
    assert_eq!(scoped.label(&second).await, ticket);
    assert_eq!(
        agent_ids(&scoped.ticket(&ticket).await),
        [first.as_str(), second.as_str()]
    );
    assert_eq!(scoped.ticket_count().await, 1);

    for unknown in [json!(TicketId::generate()), json!("not-a-ticket")] {
        let mut foreign = create("Elsewhere");
        foreign["ticketId"] = unknown;
        let refused = scoped.call(&bot, None, "create_agent", foreign).await;
        assert!(text(&refused).contains("bot_tickets"), "{refused}");
    }
    let closed = scoped
        .call(
            &bot,
            None,
            "bot_ticket_update",
            json!({"ticketId":ticket,"status":"done","outcome":"Fixed"}),
        )
        .await;
    assert_eq!(structured(&closed)["ticket"]["status"], "done");
    assert_eq!(structured(&closed)["changed"], true);
    let refused = scoped.call(&bot, None, "create_agent", joining).await;
    assert!(text(&refused).contains("closed"), "{refused}");
    assert_eq!(
        scoped.ticket(&ticket).await["assignments"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    scoped.stop().await;
}

#[tokio::test]
async fn subtree_children_inherit_the_ticket_and_nobody_else_chooses_one() {
    let (scoped, _, bot) = Scoped::ticketed().await;
    let (worker, ticket) = scoped.started(&bot, create("Fix login")).await;
    let credential = scoped.session_credential(&worker).await;
    let (child, inherited) = scoped.started(&credential, create("Helper")).await;
    assert_eq!(inherited, ticket);
    assert_eq!(scoped.label(&child).await, ticket);
    assert_eq!(
        agent_ids(&scoped.ticket(&ticket).await),
        [worker.as_str(), child.as_str()]
    );

    // Neither subtree Agents nor the host can name a ticket or write its label.
    let other = TicketId::generate();
    let mut named = create("Named");
    named["ticketId"] = json!(other);
    for token in [credential.as_str(), TOKEN] {
        rejected(
            &scoped
                .call(token, None, "create_agent", named.clone())
                .await,
            "Arguments do not match the tool input schema",
        );
    }
    let mut forged = create("Forged");
    forged["labels"] = json!({BOT_TICKET_LABEL:other});
    let mut host = forged.clone();
    host["workspaceId"] = json!(scoped.workspace);
    forbidden(&scoped.call(&credential, None, "create_agent", forged).await);
    forbidden(&scoped.call(TOKEN, None, "create_agent", host).await);
    for token in [credential.as_str(), TOKEN] {
        forbidden(
            &scoped
                .call(
                    token,
                    None,
                    "update_agent",
                    json!({"agentId":child,"labels":{BOT_TICKET_LABEL:other}}),
                )
                .await,
        );
    }
    assert_eq!(scoped.label(&child).await, ticket);
    assert!(BOT_TICKET_LABEL.starts_with(BOT_LABEL_PREFIX));

    // A child started while the Bot is paused keeps the label without an assignment; its first
    // report after the Bot resumes assigns it.
    scoped.bot("bot.pause.request").await;
    let (late, _) = scoped.started(&credential, create("Late helper")).await;
    assert_eq!(scoped.label(&late).await, ticket);
    scoped.bot("bot.resume.request").await;
    assert_eq!(
        scoped.ticket(&ticket).await["assignments"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let late_credential = scoped.session_credential(&late).await;
    let reported = scoped
        .call(
            &late_credential,
            None,
            "report_progress",
            json!({"kind":"progress","text":"started"}),
        )
        .await;
    assert_eq!(structured(&reported)["ticketStatus"], "open");
    assert_eq!(
        agent_ids(&scoped.ticket(&ticket).await),
        [worker.as_str(), child.as_str(), late.as_str()]
    );
    scoped.stop().await;
}

/// The `create_agent` input schema properties `bearer` sees.
async fn create_agent_properties(scoped: &Scoped, bearer: &str) -> Value {
    let listed: Value = scoped
        .post(bearer, None, "tools/list", json!({}))
        .await
        .json()
        .await
        .unwrap();
    listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "create_agent")
        .map(|tool| tool["inputSchema"]["properties"].clone())
        .unwrap()
}

#[tokio::test]
async fn ticket_tools_are_visible_only_to_their_callers() {
    let (scoped, _, bot) = Scoped::ticketed().await;
    let (worker, _) = scoped.started(&bot, create("Fix login")).await;
    let worker = scoped.session_credential(&worker).await;
    let plain = scoped.subtree_credential(&scoped.subtree_agent().await);
    let bot_tools = ["bot_tickets", "bot_ticket_update"];
    let listed = scoped.tools(&bot).await;
    assert!(bot_tools.iter().all(|tool| listed.contains(*tool)));
    assert!(!listed.contains("report_progress"));
    let listed = scoped.tools(&worker).await;
    assert!(listed.contains("report_progress"));
    assert!(bot_tools.iter().all(|tool| !listed.contains(*tool)));
    for bearer in [plain.as_str(), TOKEN] {
        let listed = scoped.tools(bearer).await;
        for tool in bot_tools.iter().chain(&["report_progress"]) {
            assert!(!listed.contains(*tool), "{tool}");
        }
        rejected(
            &scoped
                .call(
                    bearer,
                    None,
                    "report_progress",
                    json!({"kind":"done","text":"x"}),
                )
                .await,
            "Unknown or unavailable tool",
        );
    }
    rejected(
        &scoped
            .call(
                &bot,
                None,
                "report_progress",
                json!({"kind":"done","text":"x"}),
            )
            .await,
        "Unknown or unavailable tool",
    );
    assert!(create_agent_properties(&scoped, &bot).await["ticketId"].is_object());
    for bearer in [worker.as_str(), plain.as_str(), TOKEN] {
        assert!(
            create_agent_properties(&scoped, bearer)
                .await
                .get("ticketId")
                .is_none()
        );
    }
    scoped.stop().await;
}

#[tokio::test]
async fn worker_reports_are_recorded_and_only_outcomes_notify_the_bot() {
    let (scoped, conversation, bot) = Scoped::ticketed().await;
    let (agent, ticket) = scoped.started(&bot, create("Fix login")).await;
    let worker = scoped.session_credential(&agent).await;
    let report = |kind: &str, text: &str| json!({"kind":kind,"text":text});
    let progress = scoped
        .call(
            &worker,
            None,
            "report_progress",
            report("progress", "found the bug"),
        )
        .await;
    assert_eq!(structured(&progress)["ticketStatus"], "open");
    assert_eq!(structured(&progress)["ticketId"], ticket);
    assert!(scoped.notification_kinds().await.is_empty());
    let blocked = scoped
        .call(
            &worker,
            None,
            "report_progress",
            report("blocked", "which account?"),
        )
        .await;
    assert_eq!(structured(&blocked)["ticketStatus"], "waiting");
    let done = scoped
        .call(&worker, None, "report_progress", report("done", "fixed"))
        .await;
    assert_eq!(structured(&done)["ticketStatus"], "open");
    let failed = scoped
        .call(
            &worker,
            None,
            "report_progress",
            report("failed", "gave up"),
        )
        .await;
    assert!(succeeded(&failed), "{failed}");
    assert_eq!(
        scoped.notification_kinds().await,
        ["ticket_blocked", "ticket_done", "ticket_failed"]
    );
    let listed = scoped
        .rpc(
            "bot.notifications.list.request",
            json!({"workspaceId":scoped.workspace}),
        )
        .await;
    for entry in listed["notifications"].as_array().unwrap() {
        let notification = &entry["notification"];
        assert_eq!(notification["conversationId"], conversation.to_string());
        assert_eq!(notification["ticketId"], ticket);
        assert_eq!(notification["ticketTitle"], "Fix login");
        assert_eq!(notification["agentId"], agent);
    }
    let reports = scoped.ticket(&ticket).await["reports"].clone();
    let kinds: Vec<_> = reports
        .as_array()
        .unwrap()
        .iter()
        .map(|report| report["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["progress", "blocked", "done", "failed"]);

    // A closed ticket refuses reports with guidance instead of a bare conflict.
    scoped
        .call(
            &bot,
            None,
            "bot_ticket_update",
            json!({"ticketId":ticket,"status":"cancelled"}),
        )
        .await;
    let refused = scoped
        .call(&worker, None, "report_progress", report("done", "late"))
        .await;
    assert!(text(&refused).contains("not recorded"), "{refused}");
    scoped.stop().await;
}

#[tokio::test]
async fn bot_ticket_tools_list_close_and_guide() {
    let (scoped, _, bot) = Scoped::ticketed().await;
    let (_, ticket) = scoped.started(&bot, create("Fix login")).await;
    let listed = scoped.call(&bot, None, "bot_tickets", json!({})).await;
    assert_eq!(structured(&listed)["tickets"][0]["id"], ticket);
    let update = |arguments: Value| scoped.call(&bot, None, "bot_ticket_update", arguments);
    let renamed =
        update(json!({"ticketId":ticket,"title":"Fix SSO login","note":"user asked"})).await;
    assert_eq!(structured(&renamed)["ticket"]["title"], "Fix SSO login");
    let empty = update(json!({"ticketId":ticket})).await;
    assert!(text(&empty).contains("at least one"), "{empty}");
    let stray = update(json!({"ticketId":ticket,"status":"open","outcome":"x"})).await;
    assert!(text(&stray).contains("at least one"), "{stray}");
    let unknown = update(json!({"ticketId":TicketId::generate(),"status":"done"})).await;
    assert!(text(&unknown).contains("bot_tickets"), "{unknown}");
    update(json!({"ticketId":ticket,"status":"done"})).await;
    let conflict = update(json!({"ticketId":ticket,"status":"failed"})).await;
    assert!(text(&conflict).contains("reopen"), "{conflict}");
    let open = scoped.call(&bot, None, "bot_tickets", json!({})).await;
    assert_eq!(structured(&open)["tickets"], json!([]));
    let closed = scoped
        .call(&bot, None, "bot_tickets", json!({"includeClosed":true}))
        .await;
    assert_eq!(structured(&closed)["tickets"][0]["status"], "done");
    scoped.stop().await;
}

#[tokio::test]
async fn overview_lists_open_tickets_with_live_agent_status() {
    let (scoped, conversation, bot) = Scoped::ticketed().await;
    let (agent, ticket) = scoped.started(&bot, create("Fix login")).await;
    // A session of another conversation; reissuing replaces the hidden session's credential.
    let (_, foreign) = scoped
        .started(&scoped.bot_credential(), create("Other task"))
        .await;
    let bot = scoped.bot_credential_in(conversation);
    let worker = scoped.session_credential(&agent).await;
    scoped
        .call(
            &worker,
            None,
            "report_progress",
            json!({"kind":"progress","text":"halfway"}),
        )
        .await;
    let overview = scoped
        .call(&bot, None, "workspace_overview", json!({}))
        .await;
    let tickets = structured(&overview)["tickets"].as_array().unwrap().clone();
    assert_eq!(tickets.len(), 2, "{tickets:?}");
    let listed = tickets.iter().find(|entry| entry["id"] == ticket).unwrap();
    assert_eq!(listed["title"], "Fix login");
    assert_eq!(listed["status"], "open");
    assert_eq!(listed["thisConversation"], true);
    assert_eq!(listed["agents"][0]["id"], agent);
    let live = scoped
        .rpc("agent.get.request", json!({"agentId":agent}))
        .await["agent"]["status"]
        .clone();
    assert!(live.is_string());
    assert_eq!(listed["agents"][0]["status"], live);
    assert_eq!(listed["latestReport"]["kind"], "progress");
    assert_eq!(listed["latestReport"]["text"], "halfway");
    let other = tickets.iter().find(|entry| entry["id"] == foreign).unwrap();
    assert_eq!(other["thisConversation"], false);

    scoped
        .call(
            &bot,
            None,
            "bot_ticket_update",
            json!({"ticketId":ticket,"status":"done"}),
        )
        .await;
    let overview = scoped
        .call(&bot, None, "workspace_overview", json!({}))
        .await;
    let tickets = &structured(&overview)["tickets"];
    assert_eq!(tickets.as_array().unwrap().len(), 1, "{tickets}");
    assert_eq!(tickets[0]["id"], foreign);
    scoped.stop().await;
}

/// Wait until `conversation` has a wake turn naming a ticket, and return it.
async fn wake_turn(scoped: &Scoped, conversation: ConversationId) -> Value {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let history = scoped
                .rpc(
                    "bot.history.get.request",
                    json!({"workspaceId":scoped.workspace,"conversationId":conversation,
                        "limit":50}),
                )
                .await;
            let woken = history["turns"]
                .as_array()
                .unwrap()
                .iter()
                .find(|turn| turn["wake"]["notifications"].is_array())
                .cloned();
            if let Some(turn) = woken {
                return turn;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn a_worker_reporting_done_wakes_the_bot_in_the_conversation_that_asked() {
    let (scoped, conversation, bot) = Scoped::ticketed().await;
    scoped.turns.run();
    scoped.turns.release();
    scoped
        .rpc(
            "bot.message.send.request",
            json!({"workspaceId":scoped.workspace,"conversationId":conversation,
                "messageId":"u1","text":"please fix the login"}),
        )
        .await;
    // The Bot's turn delegates the work and ends.
    let (agent, ticket) = scoped.started(&bot, create("Fix login")).await;
    let worker = scoped.session_credential(&agent).await;
    let done = scoped
        .call(
            &worker,
            None,
            "report_progress",
            json!({"kind":"done","text":"login fixed"}),
        )
        .await;
    assert!(succeeded(&done), "{done}");

    let turn = wake_turn(&scoped, conversation).await;
    let woke = &turn["wake"]["notifications"][0];
    assert_eq!(woke["kind"], "ticket_done");
    assert_eq!(woke["ticketId"], ticket);
    assert_eq!(woke["ticketTitle"], "Fix login");
    assert_eq!(woke["summary"], "login fixed");
    assert!(
        turn["text"].as_str().unwrap().contains(&format!(
            "[ticket {} \"Fix login\"]",
            ticket.as_str().unwrap()
        )),
        "{turn}"
    );
    scoped.stop().await;
}

impl Scoped {
    /// Open `count` tickets without Agents, as a crash after opening leaves them.
    async fn fill_tickets(&self, count: usize) {
        for index in 0..count {
            let opened = self
                .bots
                .open_ticket(::bot::tickets::OpenTicket {
                    root: self.workspace.clone(),
                    draft: domain::bot::ticket::TicketDraft {
                        conversation_id: None,
                        key: format!("fill-{index}-{}", TicketId::generate()),
                        title: format!("Leftover {index}"),
                        request: String::new(),
                    },
                })
                .await
                .unwrap();
            assert!(opened.created);
        }
    }

    async fn report(&self, agent: &str, kind: &str, text: &str) -> Value {
        let worker = self.session_credential(agent).await;
        self.call(
            &worker,
            None,
            "report_progress",
            json!({"kind":kind,"text":text}),
        )
        .await
    }

    async fn archive(&self, agent: &str) {
        self.rpc("agent.archive.request", json!({"agentId":agent}))
            .await;
    }
}

#[tokio::test]
async fn the_open_limit_closes_tickets_whose_agents_are_all_archived_before_refusing() {
    let (scoped, _, bot) = Scoped::ticketed().await;
    let (finished, done_ticket) = scoped.started(&bot, create("Finished")).await;
    assert!(succeeded(&scoped.report(&finished, "done", "merged").await));
    scoped.archive(&finished).await;
    let (abandoned, cancelled_ticket) = scoped.started(&bot, create("Abandoned")).await;
    scoped.archive(&abandoned).await;
    let (blocked, waiting_ticket) = scoped.started(&bot, create("Blocked")).await;
    assert!(succeeded(
        &scoped.report(&blocked, "blocked", "which account?").await
    ));
    scoped.archive(&blocked).await;
    let (_, live_ticket) = scoped.started(&bot, create("Still running")).await;
    // One Agent still running keeps its ticket open with the other archived.
    let (first, family_ticket) = scoped.started(&bot, create("Pair")).await;
    let mut joining = create("Second of the pair");
    joining["ticketId"] = family_ticket.clone();
    scoped.started(&bot, joining).await;
    scoped.archive(&first).await;
    scoped.fill_tickets(27).await;

    let (_, overflow) = scoped.started(&bot, create("Overflow")).await;

    let done = scoped.ticket(&done_ticket).await;
    assert_eq!(done["status"], "done");
    assert!(
        done["outcome"].as_str().unwrap().contains("automatically"),
        "{done}"
    );
    let last = done["reports"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(
        (last["source"].as_str(), last["kind"].as_str()),
        (Some("system"), Some("status"))
    );
    assert_eq!(
        scoped.ticket(&cancelled_ticket).await["status"],
        "cancelled"
    );
    // A ticket waiting on a blocked worker nobody can answer any more is cancelled.
    let cancelled = scoped.ticket(&waiting_ticket).await;
    assert_eq!(cancelled["status"], "cancelled");
    assert!(
        cancelled["outcome"].as_str().unwrap().contains("blocked"),
        "{cancelled}"
    );
    assert_eq!(scoped.ticket(&live_ticket).await["status"], "open");
    assert_eq!(scoped.ticket(&family_ticket).await["status"], "open");
    assert_eq!(scoped.ticket(&overflow).await["status"], "open");

    // Nothing else is evidently over: the next ticket beyond the limit is refused with guidance.
    scoped.fill_tickets(2).await;
    let refused = scoped
        .call(&bot, None, "create_agent", create("One too many"))
        .await;
    let guidance = text(&refused);
    assert!(guidance.contains("bot_ticket_update"), "{guidance}");
    assert!(
        guidance.contains("finished or no longer needed"),
        "{guidance}"
    );
    scoped.stop().await;
}

#[tokio::test]
async fn giving_an_agent_more_work_reopens_its_closed_ticket() {
    let (scoped, _, bot) = Scoped::ticketed().await;
    let (agent, ticket) = scoped.started(&bot, create("Fix login")).await;
    let close = json!({"ticketId":ticket,"status":"done","outcome":"Fixed"});
    scoped
        .call(&bot, None, "bot_ticket_update", close.clone())
        .await;

    let prompted = scoped
        .call(
            &bot,
            None,
            "send_agent_prompt",
            json!({"agentId":agent,"prompt":"also fix the logout page"}),
        )
        .await;

    assert!(succeeded(&prompted), "{prompted}");
    let reopened = scoped.ticket(&ticket).await;
    assert_eq!(reopened["status"], "open");
    let last = reopened["reports"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(last["source"], "system");
    assert!(last["text"].as_str().unwrap().contains(&agent), "{last}");
    assert!(succeeded(
        &scoped.report(&agent, "done", "logout fixed").await
    ));

    assert_eq!(structured(&prompted).get("ticketNote"), None);

    // At the open ticket limit, tickets whose Agents are all archived are closed to make room.
    scoped
        .call(&bot, None, "bot_ticket_update", close.clone())
        .await;
    let (archived, finished) = scoped.started(&bot, create("Finished")).await;
    scoped.archive(&archived).await;
    scoped.fill_tickets(31).await;
    let prompted = scoped
        .call(
            &bot,
            None,
            "send_agent_prompt",
            json!({"agentId":agent,"prompt":"and the signup page"}),
        )
        .await;
    assert_eq!(structured(&prompted).get("ticketNote"), None);
    assert_eq!(scoped.ticket(&ticket).await["status"], "open");
    assert_eq!(scoped.ticket(&finished).await["status"], "cancelled");

    // With nothing to close, the prompt still goes through and tells the Bot what happened.
    scoped.call(&bot, None, "bot_ticket_update", close).await;
    scoped.fill_tickets(1).await;
    let prompted = scoped
        .call(
            &bot,
            None,
            "send_agent_prompt",
            json!({"agentId":agent,"prompt":"and the password page"}),
        )
        .await;
    let note = structured(&prompted)["ticketNote"]
        .as_str()
        .unwrap_or_default();
    assert!(note.contains("bot_ticket_update"), "{prompted}");
    assert!(note.contains(ticket.as_str().unwrap()), "{prompted}");
    assert_eq!(scoped.ticket(&ticket).await["status"], "done");
    scoped.stop().await;
}

#[tokio::test]
async fn an_agent_on_a_full_ticket_is_still_heard() {
    let (scoped, _, bot) = Scoped::ticketed().await;
    let (worker, ticket) = scoped.started(&bot, create("Fix login")).await;
    let ticket_id: TicketId = ticket.as_str().unwrap().parse().unwrap();
    for index in 2..=16 {
        scoped
            .bots
            .assign_ticket(::bot::tickets::AssignTicket {
                root: scoped.workspace.clone(),
                ticket_id,
                agent_id: format!("agent-{index}"),
                workspace_id: scoped.workspace.clone(),
            })
            .await
            .unwrap();
    }
    let credential = scoped.session_credential(&worker).await;
    let (child, inherited) = scoped.started(&credential, create("Helper")).await;
    assert_eq!(inherited, ticket);

    let reported = scoped.report(&child, "done", "my part is done").await;

    assert_eq!(structured(&reported)["ticketStatus"], "open");
    let stored = scoped.ticket(&ticket).await;
    assert_eq!(stored["assignments"].as_array().unwrap().len(), 16);
    let last = stored["reports"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(
        (last["agentId"].as_str(), last["kind"].as_str()),
        (Some(child.as_str()), Some("done"))
    );
    assert_eq!(scoped.notification_kinds().await, ["ticket_done"]);
    scoped.stop().await;
}
