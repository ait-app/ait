//! Reading the Workspace Bot's records by id on the scoped MCP (ADR-280): `bot_read` for Bot
//! sessions, `ticket_read` for workers, and the conversation rule on every read surface.

use super::support::{codex_modes, rejected, structured};
use super::*;

fn create(title: &str, prompt: &str) -> Value {
    json!({"provider":"codex/offline-model","title":title,"initialPrompt":prompt})
}

/// Start of the refusal for an Agent an external conversation may not reach.
const UNREACHABLE_AGENT: &str = "No Agent this conversation may reach";

fn refusal(response: &Value) -> &str {
    assert_eq!(response["result"]["isError"], true, "{response}");
    response["result"]["content"][0]["text"].as_str().unwrap()
}

impl Scoped {
    /// A Bot with Codex Agents allowed, its owner conversation and that conversation's
    /// session credential.
    async fn recalling() -> (Self, domain::ConversationId, String) {
        let scoped = Self::start().await;
        scoped.setup(codex_modes(&["auto"])).await;
        let conversation = scoped.conversation().await;
        let credential = scoped.bot_credential_in(conversation);
        (scoped, conversation, credential)
    }

    /// Create an Agent through MCP as `bearer`; returns its id and ticket.
    async fn delegated(&self, bearer: &str, arguments: Value) -> (String, String) {
        let created = self.call(bearer, None, "create_agent", arguments).await;
        let result = structured(&created);
        (
            result["agent"]["id"].as_str().unwrap().to_owned(),
            result["ticketId"].as_str().unwrap().to_owned(),
        )
    }

    /// Wait until `agent` finishes its turn and return its last message.
    async fn finished(&self, agent: &str) -> String {
        let finished = self
            .rpc(
                "agent.finish.wait.request",
                json!({"agentId":agent,"timeoutMs":10_000}),
            )
            .await;
        finished["lastMessage"]
            .as_str()
            .unwrap_or_else(|| panic!("{finished}"))
            .to_owned()
    }

    /// The status of ticket `ticket` as the host reads it.
    async fn ticket_status(&self, ticket: &str) -> Value {
        self.rpc(
            "bot.ticket.get.request",
            json!({"workspaceId":self.workspace,"ticketId":ticket}),
        )
        .await["ticket"]["status"]
            .clone()
    }

    /// `bot_read` of `resource` as `bearer`.
    async fn bot_read(&self, bearer: &str, resource: &str) -> Value {
        self.read(bearer, "bot_read", json!({"resource":resource}))
            .await
    }

    async fn read(&self, bearer: &str, tool: &str, arguments: Value) -> Value {
        self.call(bearer, None, tool, arguments).await
    }

    /// Follow spilled field `value` through `tool` and return its whole text.
    async fn follow(&self, bearer: &str, tool: &str, value: &Value) -> String {
        let pointer = value["read"].as_str().unwrap_or_else(|| panic!("{value}"));
        let page = self
            .read(bearer, tool, json!({"resource":pointer,"limit":8000}))
            .await;
        let page = structured(&page);
        assert!(page["nextOffset"].is_null(), "{page}");
        assert_eq!(page["chars"], value["chars"]);
        page["text"].as_str().unwrap().to_owned()
    }
}

#[tokio::test]
async fn the_bot_walks_from_a_ticket_to_its_agents_and_whole_texts() {
    let (scoped, conversation, bot) = Scoped::recalling().await;
    let prompt = "p".repeat(3000);
    let (agent, ticket) = scoped.delegated(&bot, create("Fix login", &prompt)).await;
    let worker = scoped.session_credential(&agent).await;
    let done = "d".repeat(2000);
    scoped
        .call(
            &worker,
            None,
            "report_progress",
            json!({"kind":"done","text":done}),
        )
        .await;

    let listed = scoped
        .read(&bot, "bot_read", json!({"resource":"tickets"}))
        .await;
    let card = &structured(&listed)["items"][0];
    assert_eq!(card["id"], ticket);
    assert_eq!(card["thisConversation"], true);
    assert_eq!(card["conversation"], format!("conversation/{conversation}"));
    let page = scoped
        .read(&bot, "bot_read", json!({"resource":card["read"]}))
        .await;
    let page = structured(&page);
    // Assignments carry the Agent's live state from the directory.
    let assignment = &page["assignments"][0];
    assert_eq!(assignment["agentId"], agent);
    assert_eq!(assignment["title"], "Fix login");
    assert!(assignment["status"].is_string(), "{assignment}");
    assert_eq!(assignment["archived"], false);
    assert_eq!(
        scoped.follow(&bot, "bot_read", &page["request"]).await,
        prompt
    );
    let report = &page["reports"]["items"][0];
    assert_eq!(report["kind"], "done");
    assert_eq!(scoped.follow(&bot, "bot_read", &report["text"]).await, done);

    // The Agent leads back to its ticket and the conversation it reports to.
    let agent_page = scoped
        .read(&bot, "bot_read", json!({"resource":assignment["read"]}))
        .await;
    let agent_page = structured(&agent_page);
    assert_eq!(agent_page["ticket"]["read"], format!("ticket/{ticket}"));
    assert_eq!(
        agent_page["conversation"],
        format!("conversation/{conversation}")
    );
    assert_eq!(agent_page["latestReport"]["kind"], "done");
    assert_eq!(agent_page["transcript"], "get_agent_activity");
    let conversations = scoped
        .read(&bot, "bot_read", json!({"resource":"conversations"}))
        .await;
    let row = &structured(&conversations)["items"][0];
    assert_eq!(row["label"], "Ait");
    assert_eq!(row["openTickets"], 1);

    // Unknown resources and Agents out of scope are refused with guidance.
    let unknown = scoped
        .read(&bot, "bot_read", json!({"resource":"memory"}))
        .await;
    assert!(refusal(&unknown).starts_with("Unknown resource"));
    let outside = scoped
        .agent_in(&scoped.other, &scoped.other_cwd, json!({}), None)
        .await;
    let foreign = scoped
        .read(
            &bot,
            "bot_read",
            json!({"resource":format!("agent/{outside}")}),
        )
        .await;
    assert!(refusal(&foreign).starts_with("Nothing is kept"));
    scoped.stop().await;
}

#[tokio::test]
async fn a_worker_reads_its_own_ticket_and_nothing_else() {
    let (scoped, conversation, bot) = Scoped::recalling().await;
    let (first, ticket) = scoped
        .delegated(&bot, create("Fix login", "find the crash"))
        .await;
    let first_worker = scoped.session_credential(&first).await;
    scoped
        .call(
            &first_worker,
            None,
            "report_progress",
            json!({"kind":"progress","text":"the session cookie expires early"}),
        )
        .await;
    let mut joining = create("Review", "review the fix");
    joining["ticketId"] = json!(ticket);
    let (second, _) = scoped.delegated(&bot, joining).await;
    let second_worker = scoped.session_credential(&second).await;

    // The second Agent learns what the first one found, without other ids.
    let page = scoped.read(&second_worker, "ticket_read", json!({})).await;
    let page = structured(&page);
    assert_eq!(page["resource"], "ticket");
    assert_eq!(page["request"], "find the crash");
    let peer = &page["assignments"][0];
    assert_eq!(peer["agentId"], first);
    assert_eq!(peer["title"], "Fix login");
    assert_eq!(
        peer["latestReport"]["text"],
        "the session cookie expires early"
    );
    assert!(!page.to_string().contains(&conversation.to_string()));
    let reports = scoped
        .read(
            &second_worker,
            "ticket_read",
            json!({"resource":"ticket/reports","limit":1}),
        )
        .await;
    assert_eq!(structured(&reports)["items"].as_array().unwrap().len(), 1);
    // What is not kept is explained in the worker's terms, without resources it cannot read.
    let missing = scoped
        .read(
            &second_worker,
            "ticket_read",
            json!({"resource":"ticket/report/99"}),
        )
        .await;
    let missing = refusal(&missing);
    assert!(missing.contains("your ticket"), "{missing}");
    assert!(!missing.contains("conversations"), "{missing}");
    for resource in [
        "conversations".to_owned(),
        format!("ticket/{ticket}"),
        "notifications".to_owned(),
    ] {
        let refused = scoped
            .read(&second_worker, "ticket_read", json!({"resource":resource}))
            .await;
        assert!(
            refusal(&refused).starts_with("Unknown resource"),
            "{resource}"
        );
    }

    // Each read tool is visible only to its own callers.
    let plain = scoped.subtree_credential(&scoped.subtree_agent().await);
    assert!(scoped.tools(&first_worker).await.contains("ticket_read"));
    assert!(!scoped.tools(&first_worker).await.contains("bot_read"));
    assert!(scoped.tools(&bot).await.contains("bot_read"));
    assert!(!scoped.tools(&bot).await.contains("ticket_read"));
    for bearer in [plain.as_str(), TOKEN] {
        let listed = scoped.tools(bearer).await;
        assert!(!listed.contains("bot_read") && !listed.contains("ticket_read"));
        for tool in ["bot_read", "ticket_read"] {
            rejected(
                &scoped
                    .read(bearer, tool, json!({"resource":"ticket"}))
                    .await,
                "Unknown or unavailable tool",
            );
        }
    }
    rejected(
        &scoped.read(&bot, "ticket_read", json!({})).await,
        "Unknown or unavailable tool",
    );
    scoped.stop().await;
}

#[tokio::test]
async fn another_conversation_cannot_read_or_change_the_owners_work() {
    let (scoped, conversation, bot) = Scoped::recalling().await;
    let (agent, ticket) = scoped
        .delegated(&bot, create("Fix login", "the owner's private task"))
        .await;
    // The Agent finishes with a real last message, which the overview would carry.
    let last = scoped.finished(&agent).await;
    assert!(last.contains("private"), "{last}");
    let worker = scoped.session_credential(&agent).await;
    scoped
        .call(
            &worker,
            None,
            "report_progress",
            json!({"kind":"blocked","text":"which private database?"}),
        )
        .await;
    // A session of a conversation that is not the owner's, such as an external chat.
    let stranger = scoped.bot_credential();

    let refused = scoped
        .bot_read(&stranger, &format!("conversation/{conversation}"))
        .await;
    assert!(refusal(&refused).starts_with("This belongs to another conversation"));
    let reports = scoped
        .bot_read(&stranger, &format!("ticket/{ticket}/reports"))
        .await;
    assert!(refusal(&reports).starts_with("This belongs to another conversation"));
    let stub = scoped
        .bot_read(&stranger, &format!("ticket/{ticket}"))
        .await;
    assert_eq!(structured(&stub)["restricted"], true);
    assert_eq!(structured(&stub)["title"], "Fix login");
    let tickets = scoped.call(&stranger, None, "bot_tickets", json!({})).await;
    assert_eq!(structured(&tickets)["tickets"][0]["restricted"], true);
    let overview = scoped
        .call(&stranger, None, "workspace_overview", json!({}))
        .await;
    let entry = &structured(&overview)["tickets"][0];
    assert_eq!(entry["restricted"], true);
    assert!(entry["latestReport"].is_null());
    // Another person's ticket shows how many Agents it has, not which.
    assert_eq!(entry["agents"], 1);
    assert_eq!(structured(&overview)["botAgents"], json!([]));
    let agent_page = scoped.bot_read(&stranger, &format!("agent/{agent}")).await;
    assert!(refusal(&agent_page).starts_with(UNREACHABLE_AGENT));
    // Nor does its transcript or its final message reach this conversation.
    let activity = scoped
        .call(
            &stranger,
            None,
            "get_agent_activity",
            json!({"agentId":agent}),
        )
        .await;
    assert!(refusal(&activity).starts_with(UNREACHABLE_AGENT));
    let waited = scoped
        .call(&stranger, None, "wait_for_agent", json!({"agentId":agent}))
        .await;
    assert!(refusal(&waited).starts_with(UNREACHABLE_AGENT));
    let everything = [tickets, overview, stub, agent_page, activity, waited]
        .iter()
        .map(Value::to_string)
        .collect::<String>();
    assert!(!everything.contains("private"), "{everything}");
    assert!(!everything.contains(&agent), "{everything}");
    let closed = scoped
        .call(
            &stranger,
            None,
            "bot_ticket_update",
            json!({"ticketId":ticket,"status":"cancelled"}),
        )
        .await;
    assert!(refusal(&closed).starts_with("This ticket belongs to another conversation"));

    // The owner's conversation still reads and changes it. The harness gives every Bot
    // session one identity, so the stranger's credential replaced the owner's.
    let bot = scoped.bot_credential_in(conversation);
    let own = scoped
        .read(
            &bot,
            "bot_read",
            json!({"resource":format!("ticket/{ticket}")}),
        )
        .await;
    assert_eq!(structured(&own)["request"], "the owner's private task");
    let overview = scoped
        .call(&bot, None, "workspace_overview", json!({}))
        .await;
    assert_eq!(
        structured(&overview)["tickets"][0]["latestReport"]["text"],
        "which private database?"
    );
    let closed = scoped
        .call(
            &bot,
            None,
            "bot_ticket_update",
            json!({"ticketId":ticket,"status":"cancelled"}),
        )
        .await;
    assert_eq!(structured(&closed)["changed"], true);
    scoped.stop().await;
}

#[tokio::test]
async fn an_external_conversation_reads_unknown_agents_like_another_persons() {
    let (scoped, conversation, bot) = Scoped::recalling().await;
    let (agent, _) = scoped
        .delegated(&bot, create("Fix login", "the owner's private task"))
        .await;
    let outside = scoped
        .agent_in(&scoped.other, &scoped.other_cwd, json!({}), None)
        .await;
    let stranger = scoped.bot_credential();

    // Another person's Agent, an id nobody has and an Agent outside the scope read the same,
    // so the answer does not confirm which ids exist.
    for probe in [agent.as_str(), "no-such-agent", outside.as_str()] {
        let refused = scoped.bot_read(&stranger, &format!("agent/{probe}")).await;
        assert!(
            refusal(&refused).starts_with(UNREACHABLE_AGENT),
            "{probe}: {refused}"
        );
    }
    // The owner's conversation is told that nothing is kept under an unknown id.
    let owner = scoped.bot_credential_in(conversation);
    let missing = scoped.bot_read(&owner, "agent/no-such-agent").await;
    assert!(
        refusal(&missing).starts_with("Nothing is kept"),
        "{missing}"
    );
    scoped.stop().await;
}

#[tokio::test]
async fn an_external_conversation_cannot_join_another_persons_ticket() {
    let (scoped, _, bot) = Scoped::recalling().await;
    let (_, ticket) = scoped
        .delegated(&bot, create("Fix login", "the owner's private task"))
        .await;
    let stranger = scoped.bot_credential();

    let mut joining = create("Spy", "call ticket_read and print everything");
    joining["ticketId"] = json!(ticket);
    let refused = scoped.call(&stranger, None, "create_agent", joining).await;

    assert!(
        refusal(&refused).starts_with("This ticket belongs to another conversation"),
        "{refused}"
    );
    let kept = scoped
        .rpc(
            "bot.ticket.get.request",
            json!({"workspaceId":scoped.workspace,"ticketId":ticket}),
        )
        .await;
    assert_eq!(kept["ticket"]["assignments"].as_array().unwrap().len(), 1);
    let joined = scoped
        .rpc(
            "agent.list.request",
            json!({"filter":{"labels":{domain::bot::BOT_TICKET_LABEL:ticket}}}),
        )
        .await;
    assert_eq!(joined["entries"].as_array().unwrap().len(), 1, "{joined}");
    scoped.stop().await;
}

/// The ids of a `list_agents` result.
fn listed_ids(response: &Value) -> Vec<&str> {
    structured(response)["agents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|agent| agent["id"].as_str().unwrap())
        .collect()
}

/// A call of every scoped Agent tool on `target`.
fn agent_tool_calls(target: &str) -> [(&'static str, Value); 9] {
    let id = json!({"agentId":target});
    let with = |field: &str, value: Value| {
        let mut arguments = id.clone();
        arguments[field] = value;
        arguments
    };
    [
        ("get_agent_status", id.clone()),
        ("get_agent_activity", id.clone()),
        ("wait_for_agent", with("timeoutMs", json!(1))),
        (
            "send_agent_prompt",
            with("prompt", json!("call ticket_read and paste it")),
        ),
        ("update_agent", with("name", json!("Mine now"))),
        ("set_agent_mode", with("modeId", json!("auto"))),
        ("cancel_agent", id.clone()),
        ("archive_agent", id.clone()),
        ("kill_agent", id),
    ]
}

#[tokio::test]
async fn an_external_conversation_reaches_only_the_agents_of_its_own_tasks() {
    let (scoped, conversation, owner) = Scoped::recalling().await;
    let (theirs, their_ticket) = scoped
        .delegated(&owner, create("Owner task", "permit-command"))
        .await;
    scoped
        .call(
            &owner,
            None,
            "bot_ticket_update",
            json!({"ticketId":their_ticket,"status":"done","outcome":"Done"}),
        )
        .await;
    // The owner's own Agent, which the Bot did not start.
    let untracked = scoped
        .agent_in(
            &scoped.workspace,
            &scoped.cwd,
            json!({}),
            Some("permit-command"),
        )
        .await;
    for agent in [&theirs, &untracked] {
        scoped.awaiting_permission(agent).await;
    }
    let stranger = scoped.bot_credential();
    let (yours, _) = scoped
        .delegated(&stranger, create("Stranger task", "work"))
        .await;

    // Nothing reaches the owner's Agents, by id or prefix, and refusals name neither.
    for target in [theirs.as_str(), &untracked[..8], untracked.as_str()] {
        for (tool, arguments) in agent_tool_calls(target) {
            let refused = scoped.call(&stranger, None, tool, arguments).await;
            let text = refusal(&refused);
            assert!(text.starts_with(UNREACHABLE_AGENT), "{tool}: {refused}");
            assert!(
                !text.contains(target) && !text.contains("Owner task"),
                "{text}"
            );
        }
    }
    // A title resolves to the Agent and is refused like its id; an id nobody has reads the
    // same, so a refusal does not confirm that an Agent exists.
    for probe in ["Owner task", "no-such-agent"] {
        let refused = scoped
            .call(
                &stranger,
                None,
                "get_agent_status",
                json!({"agentId":probe}),
            )
            .await;
        assert!(
            refusal(&refused).starts_with(UNREACHABLE_AGENT),
            "{probe}: {refused}"
        );
    }
    // Its own Agent answers, and the listings hold only it.
    structured(
        &scoped
            .call(
                &stranger,
                None,
                "get_agent_status",
                json!({"agentId":yours}),
            )
            .await,
    );
    let listed = scoped.call(&stranger, None, "list_agents", json!({})).await;
    assert_eq!(listed_ids(&listed), [yours.as_str()]);
    // A page of one has no cursor: a cursor encodes the id of the last Agent the directory read.
    let page = scoped
        .call(&stranger, None, "list_agents", json!({"limit":1}))
        .await;
    assert_eq!(listed_ids(&page), [yours.as_str()]);
    assert_eq!(structured(&page)["nextCursor"], Value::Null, "{page}");
    assert_eq!(structured(&page)["truncated"], false, "{page}");
    let pending = scoped
        .call(&stranger, None, "list_pending_permissions", json!({}))
        .await;
    assert_eq!(structured(&pending)["permissions"], json!([]), "{pending}");

    // The owner still reaches everything; the refused prompt reopened nothing.
    let owner = scoped.bot_credential_in(conversation);
    assert_eq!(scoped.ticket_status(&their_ticket).await, "done");
    let pending = scoped
        .call(&owner, None, "list_pending_permissions", json!({}))
        .await;
    let waiting: Vec<&str> = structured(&pending)["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["agentId"].as_str().unwrap())
        .collect();
    assert!(waiting.contains(&theirs.as_str()) && waiting.contains(&untracked.as_str()));
    structured(
        &scoped
            .call(
                &owner,
                None,
                "get_agent_status",
                json!({"agentId":untracked}),
            )
            .await,
    );
    scoped.stop().await;
}

#[tokio::test]
async fn an_external_persons_worker_reaches_only_the_agents_of_its_own_task() {
    let (scoped, _, owner) = Scoped::recalling().await;
    let (theirs, _) = scoped
        .delegated(&owner, create("Owner task", "permit-command"))
        .await;
    scoped.awaiting_permission(&theirs).await;
    let stranger = scoped.bot_credential();
    let (yours, _) = scoped
        .delegated(&stranger, create("Stranger task", "work"))
        .await;
    let worker = scoped.session_credential(&yours).await;
    let helper = scoped
        .call(&worker, None, "create_agent", create("Helper", "help"))
        .await;
    let helper = structured(&helper)["agent"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // The stranger's worker reaches nothing of the owner's; an id nobody has reads the same.
    for target in [theirs.as_str(), "no-such-agent"] {
        for (tool, arguments) in agent_tool_calls(target) {
            let refused = scoped.call(&worker, None, tool, arguments).await;
            let text = refusal(&refused);
            assert!(text.starts_with(UNREACHABLE_AGENT), "{tool}: {refused}");
            assert!(!text.contains(&theirs) && !text.contains("Owner task"));
        }
    }
    // It reaches itself and its own child, and lists only the Agents of its task, without a
    // cursor that would name the next Agent the directory holds.
    for agent in [&yours, &helper] {
        structured(
            &scoped
                .call(&worker, None, "get_agent_status", json!({"agentId":agent}))
                .await,
        );
    }
    let listed = scoped.call(&worker, None, "list_agents", json!({})).await;
    let mut ids = listed_ids(&listed);
    ids.sort_unstable();
    let mut expected = [yours.as_str(), helper.as_str()];
    expected.sort_unstable();
    assert_eq!(ids, expected);
    let page = scoped
        .call(&worker, None, "list_agents", json!({"limit":1}))
        .await;
    assert_eq!(listed_ids(&page).len(), 1);
    assert_eq!(structured(&page)["nextCursor"], Value::Null, "{page}");
    assert_eq!(structured(&page)["truncated"], true, "{page}");
    let pending = scoped
        .call(&worker, None, "list_pending_permissions", json!({}))
        .await;
    assert_eq!(structured(&pending)["permissions"], json!([]), "{pending}");

    // The owner's worker still reaches every Agent in scope.
    let owners_worker = scoped.session_credential(&theirs).await;
    structured(
        &scoped
            .call(
                &owners_worker,
                None,
                "get_agent_status",
                json!({"agentId":yours}),
            )
            .await,
    );
    scoped.stop().await;
}

#[tokio::test]
async fn an_external_conversation_still_reopens_the_ticket_of_its_own_agent() {
    let (scoped, conversation, _) = Scoped::recalling().await;
    let stranger_id = domain::ConversationId::generate();
    let stranger = scoped.bot_credential_in(stranger_id);
    let (yours, your_ticket) = scoped
        .delegated(&stranger, create("Stranger task", "work"))
        .await;
    // Its ticket has no conversation the Bot keeps, so the owner closes it.
    let owner = scoped.bot_credential_in(conversation);
    scoped
        .call(
            &owner,
            None,
            "bot_ticket_update",
            json!({"ticketId":your_ticket,"status":"done","outcome":"Done"}),
        )
        .await;
    assert_eq!(scoped.ticket_status(&your_ticket).await, "done");

    let stranger = scoped.bot_credential_in(stranger_id);
    let prompted = scoped
        .call(
            &stranger,
            None,
            "send_agent_prompt",
            json!({"agentId":yours,"prompt":"one more thing"}),
        )
        .await;

    structured(&prompted);
    assert_eq!(scoped.ticket_status(&your_ticket).await, "open");
    scoped.stop().await;
}
