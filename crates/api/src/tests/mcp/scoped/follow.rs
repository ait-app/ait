//! Pull requests and questions on Bot tickets through the served scoped MCP (ADR-287): workers
//! bind the pull request they open and ask one question when blocked; the Bot binds pull
//! requests and passes the user's answer to the worker; other conversations can do neither on
//! the owner's tickets.

use std::sync::Mutex;

use ::bot::follow::{Delivered, PullReadError, PullRefusal, PullSource, Workers};
use domain::TicketId;
use domain::forge::{PullSnapshot, PullState};
use model::ErrorCode;

use super::support::{codex_modes, structured, succeeded};
use super::*;

/// A forge whose pull requests are numbers below 100 of `github.com/acme/app`.
struct Forge;

#[async_trait::async_trait]
impl PullSource for Forge {
    async fn read(&self, _workspace: &str, number: u64) -> Result<PullSnapshot, PullReadError> {
        if number >= 100 {
            return Err(PullReadError::NotFound);
        }
        Ok(PullSnapshot {
            number,
            url: format!("https://github.com/acme/app/pull/{number}"),
            title: format!("Change {number}"),
            state: PullState::Open,
            head: None,
            checks: Vec::new(),
            items: Vec::new(),
        })
    }

    async fn can_write(&self, _workspace: &str, _login: &str) -> Result<bool, PullReadError> {
        Ok(false)
    }
}

/// Every Agent is live and keeps what it is sent.
#[derive(Default)]
struct Inbox(Mutex<Vec<(String, String)>>);

#[async_trait::async_trait]
impl Workers for Inbox {
    async fn live(&self, _ticket: TicketId) -> Result<Vec<String>, ErrorCode> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .first()
            .map(|(agent, _)| vec![agent.clone()])
            .unwrap_or_default())
    }

    async fn inputs(&self, _agents: &[String]) -> Vec<String> {
        Vec::new()
    }

    async fn deliver(
        &self,
        agent: &str,
        _message_id: &str,
        text: &str,
    ) -> Result<Delivered, ErrorCode> {
        self.0
            .lock()
            .unwrap()
            .push((agent.to_owned(), text.to_owned()));
        Ok(Delivered::Started)
    }
}

fn refusal(response: &Value) -> &str {
    assert_eq!(response["result"]["isError"], true, "{response}");
    response["result"]["content"][0]["text"].as_str().unwrap()
}

impl Scoped {
    /// A Bot with Codex Agents allowed, one Agent it started, the owner conversation's
    /// credential and the Agent's worker credential.
    async fn following(&self) -> (String, String, String, String) {
        self.setup(codex_modes(&["auto"])).await;
        let bot = self.owner_credential().await;
        let created = self
            .call(
                &bot,
                None,
                "agent_create",
                json!({"provider":"codex/offline-model","title":"Fix login",
                    "initialPrompt":"work on it"}),
            )
            .await;
        let result = structured(&created);
        let agent = result["agent"]["id"].as_str().unwrap().to_owned();
        let ticket = result["ticketId"].as_str().unwrap().to_owned();
        let worker = self.session_credential(&agent).await;
        (bot, agent, ticket, worker)
    }

    async fn ticket_view(&self, ticket: &str) -> Value {
        self.rpc(
            "bot.ticket.get.request",
            json!({"workspaceId":self.workspace,"ticketId":ticket}),
        )
        .await["ticket"]
            .clone()
    }
}

#[tokio::test]
async fn a_worker_binds_the_pull_request_it_opened() {
    let scoped = Scoped::start().await;
    scoped.bots.attach_follow(
        &tokio::runtime::Handle::current(),
        Arc::new(Forge),
        Arc::new(Inbox::default()),
    );
    let (bot, _, ticket, worker) = scoped.following().await;

    let reported = scoped
        .call(
            &worker,
            None,
            "ticket_report",
            json!({"kind":"progress","text":"opened the PR","pullRequest":"#12"}),
        )
        .await;

    assert!(succeeded(&reported), "{reported}");
    let view = scoped.ticket_view(&ticket).await;
    assert_eq!(view["pulls"][0]["number"], 12);
    assert_eq!(view["pulls"][0]["boundBy"], "agent");
    assert_eq!(
        view["pulls"][0]["url"],
        "https://github.com/acme/app/pull/12"
    );
    let read = scoped.call(&worker, None, "ticket_read", json!({})).await;
    assert_eq!(structured(&read)["pulls"][0]["number"], 12);
    let overview = scoped.call(&bot, None, "bot_overview", json!({})).await;
    assert_eq!(
        structured(&overview)["tickets"][0]["pulls"],
        json!([{"number":12,"state":"open","error":null}])
    );
    let by_bot = scoped
        .call(
            &bot,
            None,
            "bot_ticket_update",
            json!({"ticketId":ticket,"pullRequest":"https://github.com/acme/app/pull/13"}),
        )
        .await;
    assert_eq!(structured(&by_bot)["ticket"]["pulls"][1]["boundBy"], "bot");
}

#[tokio::test]
async fn a_pull_request_the_forge_cannot_confirm_is_refused_and_nothing_is_recorded() {
    let scoped = Scoped::start().await;
    let (bot, _, ticket, worker) = scoped.following().await;
    let report =
        |reference: &str| json!({"kind":"progress","text":"opened the PR","pullRequest":reference});

    let unsupported = scoped
        .call(&worker, None, "ticket_report", report("12"))
        .await;
    scoped.bots.attach_follow(
        &tokio::runtime::Handle::current(),
        Arc::new(Forge),
        Arc::new(Inbox::default()),
    );
    let missing = scoped
        .call(&worker, None, "ticket_report", report("#404"))
        .await;
    let foreign = scoped
        .call(
            &worker,
            None,
            "ticket_report",
            report("https://github.com/other/repo/pull/12"),
        )
        .await;
    let unparsable = scoped
        .call(
            &bot,
            None,
            "bot_ticket_update",
            json!({"ticketId":ticket,"pullRequest":"the PR"}),
        )
        .await;

    assert_eq!(refusal(&unsupported), PullRefusal::Unsupported.message());
    assert_eq!(refusal(&missing), PullRefusal::NotFound.message());
    assert_eq!(refusal(&foreign), PullRefusal::OtherRepository.message());
    assert_eq!(refusal(&unparsable), PullRefusal::Reference.message());
    let view = scoped.ticket_view(&ticket).await;
    assert!(
        view.get("pulls").is_none_or(|pulls| pulls == &json!([])),
        "{view}"
    );
    assert!(
        !view["reports"]
            .as_array()
            .unwrap()
            .iter()
            .any(|report| report["kind"] == "progress"),
        "no report was recorded: {view}"
    );
}

#[tokio::test]
async fn a_blocked_workers_question_is_answered_by_the_bot_and_reaches_the_worker() {
    let scoped = Scoped::start().await;
    let inbox = Arc::new(Inbox::default());
    scoped.bots.attach_follow(
        &tokio::runtime::Handle::current(),
        Arc::new(Forge),
        inbox.clone(),
    );
    let (bot, agent, ticket, worker) = scoped.following().await;
    let misplaced = scoped
        .call(
            &worker,
            None,
            "ticket_report",
            json!({"kind":"progress","text":"x","question":"which?"}),
        )
        .await;
    assert!(refusal(&misplaced).starts_with("question is only for kind blocked"));
    let early = scoped
        .call(
            &bot,
            None,
            "bot_ticket_update",
            json!({"ticketId":ticket,"answer":{"agentId":agent,"text":"staging"}}),
        )
        .await;
    assert!(refusal(&early).starts_with("That agent is not waiting"));

    let blocked = scoped
        .call(
            &worker,
            None,
            "ticket_report",
            json!({"kind":"blocked","text":"need credentials","question":"Which account?"}),
        )
        .await;
    assert!(succeeded(&blocked), "{blocked}");
    let overview = scoped.call(&bot, None, "bot_overview", json!({})).await;
    assert_eq!(
        structured(&overview)["tickets"][0]["waiting"]["question"],
        "Which account?"
    );
    inbox.0.lock().unwrap().push((agent.clone(), String::new()));
    let stale = scoped
        .call(
            &bot,
            None,
            "bot_ticket_update",
            json!({"ticketId":ticket,"answer":{"agentId":"agt_someone_else","text":"x"}}),
        )
        .await;
    assert!(refusal(&stale).starts_with("That agent is not waiting"));
    let answered = scoped
        .call(
            &bot,
            None,
            "bot_ticket_update",
            json!({"ticketId":ticket,"answer":{"agentId":agent,"text":"the staging one"}}),
        )
        .await;

    let result = structured(&answered);
    assert_eq!(result["delivered"], true);
    assert_eq!(result["ticket"]["status"], "open");
    let last = result["ticket"]["reports"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(
        (last["source"].clone(), last["kind"].clone()),
        (json!("bot"), json!("answer"))
    );
    let sent = inbox.0.lock().unwrap().clone();
    let (to, text) = sent.last().unwrap();
    assert_eq!(to, &agent);
    assert!(text.contains("on the user's behalf"), "{text}");
    assert!(text.contains("Your question: Which account?"), "{text}");
}

#[tokio::test]
async fn another_conversation_cannot_bind_or_answer_on_the_owners_ticket() {
    let scoped = Scoped::start().await;
    scoped.bots.attach_follow(
        &tokio::runtime::Handle::current(),
        Arc::new(Forge),
        Arc::new(Inbox::default()),
    );
    let (_, _, ticket, worker) = scoped.following().await;
    scoped
        .call(
            &worker,
            None,
            "ticket_report",
            json!({"kind":"blocked","text":"need a decision","question":"Ship it?"}),
        )
        .await;
    let stranger = scoped.bot_credential();

    let bound = scoped
        .call(
            &stranger,
            None,
            "bot_ticket_update",
            json!({"ticketId":ticket,"pullRequest":"12"}),
        )
        .await;
    let answered = scoped
        .call(
            &stranger,
            None,
            "bot_ticket_update",
            json!({"ticketId":ticket,"answer":{"agentId":"agt_x","text":"yes"}}),
        )
        .await;
    let listed = scoped
        .call(&stranger, None, "bot_ticket_list", json!({}))
        .await;

    assert!(refusal(&bound).starts_with("This ticket belongs to another conversation"));
    assert!(refusal(&answered).starts_with("This ticket belongs to another conversation"));
    let stub = &structured(&listed)["tickets"][0];
    assert_eq!(stub["restricted"], true);
    assert!(stub["waiting"].get("question").is_none(), "{stub}");
    assert!(stub.get("pulls").is_none(), "{stub}");
    assert_eq!(scoped.ticket_view(&ticket).await["status"], "waiting");
}

#[tokio::test]
async fn the_report_and_update_schemas_show_the_new_arguments() {
    let scoped = Scoped::start().await;
    let (bot, _, _, worker) = scoped.following().await;

    let listed: Value = scoped
        .post(&worker, None, "tools/list", json!({}))
        .await
        .json()
        .await
        .unwrap();
    let bot_listed: Value = scoped
        .post(&bot, None, "tools/list", json!({}))
        .await
        .json()
        .await
        .unwrap();

    let schema = |listed: &Value, tool: &str| {
        listed["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|candidate| candidate["name"] == tool)
            .unwrap()["inputSchema"]["properties"]
            .clone()
    };
    let report = schema(&listed, "ticket_report");
    assert!(report.get("pullRequest").is_some() && report.get("question").is_some());
    let update = schema(&bot_listed, "bot_ticket_update");
    assert!(update.get("pullRequest").is_some() && update.get("answer").is_some());
    assert_eq!(update["answer"]["required"], json!(["agentId", "text"]));
    let description = |listed: &Value, tool: &str| {
        listed["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|candidate| candidate["name"] == tool)
            .unwrap()["description"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert!(description(&bot_listed, "bot_ticket_update").contains("closes itself"));
    assert!(
        description(&bot_listed, "bot_ticket_update")
            .contains("setting a done or failed ticket to cancelled")
    );
    assert!(::bot::tickets::PULL_LIMIT_MESSAGE.contains("done or failed ticket to cancelled"));
    assert!(description(&listed, "ticket_report").contains("not instructions"));
}
