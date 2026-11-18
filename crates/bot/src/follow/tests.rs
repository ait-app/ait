//! The follower and its ports through the running Bot service (ADR-287).

use std::collections::{BTreeSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use domain::TicketId;
use domain::bot::ticket::{TicketDraft, WorkerReportKind};
use domain::forge::{AuthorAccess, PullItem, PullItemKind, PullSnapshot, PullState};
use model::ErrorCode;
use serde_json::{Value, json};

use super::{
    BACKOFF_LIMIT, Budget, Delivered, PENDING_INTERVAL, POLL_INTERVAL, PullReadError, PullRefusal,
    PullSource, READ_BUDGET, READ_WINDOW, Workers, backoff,
};
use crate::service::Bots;
use crate::test_support::{Fixture, WORKSPACE, setup};
use crate::tickets::{AssignTicket, OpenTicket, WorkerReport};

/// Reads pull requests from a script: queued results first, then the last read again.
#[derive(Default)]
struct Pulls {
    script: Mutex<VecDeque<Result<PullSnapshot, PullReadError>>>,
    last: Mutex<Option<PullSnapshot>>,
    reads: AtomicUsize,
    /// Permission lookups answer from here, failing while it is empty.
    writers: Mutex<VecDeque<bool>>,
    lookups: AtomicUsize,
}

impl Pulls {
    fn then(&self, result: Result<PullSnapshot, PullReadError>) {
        self.script.lock().unwrap().push_back(result);
    }
}

#[async_trait::async_trait]
impl PullSource for Pulls {
    async fn read(&self, _workspace: &str, _number: u64) -> Result<PullSnapshot, PullReadError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        let next = self.script.lock().unwrap().pop_front();
        match next {
            Some(Ok(snapshot)) => {
                *self.last.lock().unwrap() = Some(snapshot.clone());
                Ok(snapshot)
            }
            Some(Err(error)) => Err(error),
            None => self
                .last
                .lock()
                .unwrap()
                .clone()
                .ok_or(PullReadError::NotFound),
        }
    }

    async fn can_write(&self, _workspace: &str, _login: &str) -> Result<bool, PullReadError> {
        self.lookups.fetch_add(1, Ordering::SeqCst);
        self.writers
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| PullReadError::Failed("rate limited".to_owned()))
    }
}

/// Agents that keep each input once per message id, like the provider's durable input
/// receipts, which outlive a restart of the Bot service; a busy Agent takes nothing.
#[derive(Default)]
struct Agents {
    live: Mutex<Vec<String>>,
    tool_inputs: Mutex<Vec<String>>,
    inputs: Mutex<Vec<(String, String, String)>>,
    receipts: Arc<Mutex<BTreeSet<String>>>,
    refuse: AtomicBool,
    busy: AtomicBool,
    /// The provider takes the input but its reply is lost, as in a crash right after.
    lose_reply: AtomicBool,
    calls: AtomicUsize,
    attempts: AtomicUsize,
}

impl Agents {
    fn inputs(&self) -> Vec<(String, String, String)> {
        self.inputs.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl Workers for Agents {
    async fn live(&self, _ticket: TicketId) -> Result<Vec<String>, ErrorCode> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.live.lock().unwrap().clone())
    }

    async fn inputs(&self, _agents: &[String]) -> Vec<String> {
        self.tool_inputs.lock().unwrap().clone()
    }

    async fn deliver(
        &self,
        agent: &str,
        message_id: &str,
        text: &str,
    ) -> Result<Delivered, ErrorCode> {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        if self.refuse.load(Ordering::SeqCst) {
            return Err(ErrorCode::AgentIo);
        }
        if self.busy.load(Ordering::SeqCst) {
            return Ok(Delivered::Busy);
        }
        if self.receipts.lock().unwrap().insert(message_id.to_owned()) {
            self.inputs.lock().unwrap().push((
                agent.to_owned(),
                message_id.to_owned(),
                text.to_owned(),
            ));
        }
        if self.lose_reply.load(Ordering::SeqCst) {
            return Err(ErrorCode::AgentIo);
        }
        Ok(Delivered::Started)
    }
}

fn snapshot(items: Vec<PullItem>) -> PullSnapshot {
    PullSnapshot {
        number: 12,
        url: "https://github.com/o/r/pull/12".into(),
        title: "Fix login".into(),
        state: PullState::Open,
        head: None,
        checks: Vec::new(),
        items,
    }
}

fn comment(id: &str, body: &str, own: bool) -> PullItem {
    PullItem {
        id: id.into(),
        kind: PullItemKind::Comment,
        author: "someone".into(),
        own,
        access: AuthorAccess::Writer,
        body: body.into(),
        url: format!("https://github.com/o/r/pull/12#{id}"),
        created_at: DateTime::parse_from_rfc3339("2030-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc),
        path: None,
        line: None,
    }
}

struct Harness {
    bots: Bots,
    pulls: Arc<Pulls>,
    agents: Arc<Agents>,
}

/// A running Bot with an owner-conversation ticket on which `agent-1` works, following with
/// fresh fakes.
async fn harness(fixture: &Fixture) -> (Harness, TicketId) {
    let bots = fixture.bots();
    bots.execute("bot.setup.request", setup(WORKSPACE))
        .await
        .unwrap();
    let opened = bots
        .execute(
            "bot.conversation.open.request",
            json!({"workspaceId":WORKSPACE}),
        )
        .await
        .unwrap();
    let conversation = serde_json::from_value(opened["conversation"]["id"].clone()).unwrap();
    let ticket = bots
        .open_ticket(OpenTicket {
            root: WORKSPACE.into(),
            draft: TicketDraft {
                conversation_id: Some(conversation),
                key: "k".into(),
                title: "Fix login".into(),
                request: "Fix it".into(),
            },
        })
        .await
        .unwrap()
        .ticket_id;
    bots.assign_ticket(AssignTicket {
        root: WORKSPACE.into(),
        ticket_id: ticket,
        agent_id: "agent-1".into(),
        workspace_id: WORKSPACE.into(),
    })
    .await
    .unwrap();
    (attach(bots), ticket)
}

fn attach(bots: Bots) -> Harness {
    attach_with(bots, Agents::default())
}

/// Attach fresh pull requests and `agents`, with `agent-1` live.
fn attach_with(bots: Bots, agents: Agents) -> Harness {
    let pulls = Arc::new(Pulls::default());
    let agents = Arc::new(agents);
    agents.live.lock().unwrap().push("agent-1".into());
    bots.attach_follow(
        &tokio::runtime::Handle::current(),
        pulls.clone(),
        agents.clone(),
    );
    Harness {
        bots,
        pulls,
        agents,
    }
}

fn report(ticket: TicketId, kind: WorkerReportKind) -> WorkerReport {
    WorkerReport {
        root: WORKSPACE.into(),
        ticket_id: ticket,
        agent_id: "agent-1".into(),
        workspace_id: WORKSPACE.into(),
        kind,
        text: "working".into(),
        question: None,
        pull: None,
    }
}

/// Bind pull request 12 as the worker would: read it, then report with it.
async fn bind(harness: &Harness, ticket: TicketId) {
    let read = harness.bots.read_pull(WORKSPACE, "#12").await.unwrap();
    let mut bound = report(ticket, WorkerReportKind::Progress);
    bound.pull = Some(read);
    harness.bots.report_progress(bound).await.unwrap();
}

/// Advance the paused clock past the poll interval until `check` holds, waiting in real time
/// for the Bot worker thread between steps.
async fn advance_until(check: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !check() {
        assert!(
            std::time::Instant::now() < deadline,
            "condition met in time"
        );
        tokio::time::advance(POLL_INTERVAL + Duration::from_secs(1)).await;
        std::thread::sleep(Duration::from_millis(2));
        tokio::task::yield_now().await;
    }
}

async fn eventually(check: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !check() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("condition met in time");
}

async fn ticket_view(bots: &Bots, ticket: TicketId) -> Value {
    bots.execute(
        "bot.ticket.get.request",
        json!({"workspaceId":WORKSPACE,"ticketId":ticket}),
    )
    .await
    .unwrap()["ticket"]
        .clone()
}

async fn pull_notifications(bots: &Bots) -> usize {
    let listed = bots
        .execute(
            "bot.notifications.list.request",
            json!({"workspaceId":WORKSPACE}),
        )
        .await
        .unwrap();
    listed["notifications"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| {
            entry["notification"]["kind"]
                .as_str()
                .is_some_and(|kind| kind.starts_with("pull_"))
        })
        .count()
}

#[tokio::test]
async fn nothing_is_read_while_no_ticket_follows_a_pull_request() {
    let fixture = Fixture::new();
    let (harness, _) = harness(&fixture).await;

    tokio::time::sleep(Duration::from_millis(300)).await;

    assert_eq!(harness.pulls.reads.load(Ordering::SeqCst), 0);
    assert_eq!(harness.agents.calls.load(Ordering::SeqCst), 0);
    harness.bots.shutdown().await.unwrap();
}

#[tokio::test]
async fn binding_refuses_what_the_forge_cannot_confirm() {
    let fixture = Fixture::new();
    let bots = fixture.bots();
    assert_eq!(
        bots.read_pull(WORKSPACE, "12").await,
        Err(PullRefusal::Unsupported)
    );
    let harness = attach(bots);
    harness
        .pulls
        .then(Err(PullReadError::Unavailable("gh: not logged in".into())));
    harness.pulls.then(Err(PullReadError::NotFound));
    harness.pulls.then(Ok(snapshot(Vec::new())));

    assert_eq!(
        harness.bots.read_pull(WORKSPACE, "pull twelve").await,
        Err(PullRefusal::Reference)
    );
    assert_eq!(
        harness.bots.read_pull(WORKSPACE, "12").await,
        Err(PullRefusal::Unavailable)
    );
    assert_eq!(
        harness.bots.read_pull(WORKSPACE, "12").await,
        Err(PullRefusal::NotFound)
    );
    assert_eq!(
        harness
            .bots
            .read_pull(WORKSPACE, "https://github.com/someone/else/pull/12")
            .await,
        Err(PullRefusal::OtherRepository)
    );
    assert!(
        PullRefusal::Unavailable
            .message()
            .contains("gh auth status")
    );
    harness.bots.shutdown().await.unwrap();
}

#[tokio::test]
async fn review_comments_reach_the_worker_directly_and_once() {
    let fixture = Fixture::new();
    let (harness, ticket) = harness(&fixture).await;
    harness.pulls.then(Ok(snapshot(Vec::new())));
    harness.pulls.then(Ok(snapshot(vec![comment(
        "c1",
        "please rename the helper",
        false,
    )])));

    bind(&harness, ticket).await;
    eventually(|| !harness.agents.inputs().is_empty()).await;
    // Past the poll interval, the follower reads the same pull request again.
    tokio::time::pause();
    advance_until(|| harness.pulls.reads.load(Ordering::SeqCst) >= 4).await;

    let inputs = harness.agents.inputs();
    assert_eq!(inputs.len(), 1, "a later read delivers nothing again");
    let (agent, id, text) = &inputs[0];
    assert_eq!(agent, "agent-1");
    assert!(id.starts_with(&format!("follow:{ticket}:")));
    assert!(text.starts_with("<ait-input kind=\"ticket_pull\""));
    assert!(text.contains("please rename the helper"));
    assert_eq!(
        pull_notifications(&harness.bots).await,
        0,
        "the Bot is not woken"
    );
    harness.bots.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_comment_the_worker_wrote_is_not_fed_back_but_the_owners_is() {
    let fixture = Fixture::new();
    let (harness, ticket) = harness(&fixture).await;
    harness
        .agents
        .tool_inputs
        .lock()
        .unwrap()
        .push("gh pr comment 12 --body 'Addressed every review comment in 1a2b3c'".into());
    harness.pulls.then(Ok(snapshot(Vec::new())));
    harness.pulls.then(Ok(snapshot(vec![
        comment("mine", "Addressed every review comment in 1a2b3c", true),
        comment("owner", "Please also cover the logout path", true),
    ])));

    bind(&harness, ticket).await;
    eventually(|| !harness.agents.inputs().is_empty()).await;

    let text = &harness.agents.inputs()[0].2;
    assert!(text.contains("Please also cover the logout path"));
    assert!(text.contains("this may be your own comment"));
    assert!(!text.contains("Addressed every review comment"));
    harness.bots.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_forge_failure_is_recorded_on_the_binding_and_never_fails_the_ticket() {
    let fixture = Fixture::new();
    let (harness, ticket) = harness(&fixture).await;
    harness.pulls.then(Ok(snapshot(Vec::new())));
    harness.pulls.then(Err(PullReadError::Failed(
        "HTTP 502 from api.github.com".into(),
    )));

    bind(&harness, ticket).await;
    let mut view = ticket_view(&harness.bots, ticket).await;
    for _ in 0..500 {
        if !view["pulls"][0]["error"].is_null() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
        view = ticket_view(&harness.bots, ticket).await;
    }

    assert_eq!(view["pulls"][0]["error"], "HTTP 502 from api.github.com");
    assert_eq!(view["status"], "open");
    harness.bots.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_owners_answer_reaches_the_blocked_worker() {
    let fixture = Fixture::new();
    let (harness, ticket) = harness(&fixture).await;
    let mut blocked = report(ticket, WorkerReportKind::Blocked);
    blocked.question = Some("Which region?".into());
    harness.bots.report_progress(blocked).await.unwrap();

    let answered = harness
        .bots
        .execute(
            "bot.ticket.answer.request",
            json!({"workspaceId":WORKSPACE,"ticketId":ticket,"agentId":"agent-1",
                "answer":"eu-west-1"}),
        )
        .await
        .unwrap();

    assert_eq!(answered["delivered"], true);
    assert_eq!(answered["ticket"]["status"], "open");
    let inputs = harness.agents.inputs();
    assert_eq!(inputs.len(), 1);
    assert!(inputs[0].1.starts_with(&format!("answer:{ticket}:")));
    assert!(inputs[0].2.starts_with("<ait-input kind=\"ticket_answer\""));
    assert!(inputs[0].2.contains("Your question: Which region?"));
    assert!(inputs[0].2.contains("Answer: eu-west-1"));
    harness.bots.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_undelivered_input_is_delivered_once_after_a_restart() {
    let fixture = Fixture::new();
    let (first, ticket) = harness(&fixture).await;
    first.agents.refuse.store(true, Ordering::SeqCst);
    first.pulls.then(Ok(snapshot(Vec::new())));
    first.pulls.then(Ok(snapshot(vec![comment(
        "c1",
        "please add a test",
        false,
    )])));
    bind(&first, ticket).await;
    eventually(|| first.pulls.reads.load(Ordering::SeqCst) >= 2).await;
    let mut pending = first.bots.follow_targets();
    tokio::time::timeout(
        Duration::from_secs(10),
        pending.wait_for(|targets| targets.iter().any(|target| target.pending)),
    )
    .await
    .unwrap()
    .unwrap();
    first.bots.shutdown().await.unwrap();

    let second = attach(fixture.bots());
    second.pulls.then(Ok(snapshot(vec![comment(
        "c1",
        "please add a test",
        false,
    )])));
    eventually(|| !second.agents.inputs().is_empty()).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let inputs = second.agents.inputs();
    assert_eq!(
        inputs.len(),
        1,
        "delivered once, not again from the second read"
    );
    assert!(inputs[0].2.contains("please add a test"));
    let mut settled = second.bots.follow_targets();
    tokio::time::timeout(
        Duration::from_secs(10),
        settled.wait_for(|targets| targets.iter().all(|target| !target.pending)),
    )
    .await
    .unwrap()
    .unwrap();
    second.bots.shutdown().await.unwrap();
}

#[test]
fn failures_back_off_up_to_the_limit() {
    assert_eq!(backoff(1), POLL_INTERVAL * 2);
    assert_eq!(backoff(2), POLL_INTERVAL * 4);
    assert_eq!(backoff(10), BACKOFF_LIMIT);
    assert_eq!(backoff(u32::MAX), BACKOFF_LIMIT);
}

#[tokio::test]
async fn an_input_waits_while_the_worker_is_busy_and_is_delivered_once_it_is_idle() {
    let fixture = Fixture::new();
    let (harness, ticket) = harness(&fixture).await;
    harness.agents.busy.store(true, Ordering::SeqCst);
    harness.pulls.then(Ok(snapshot(Vec::new())));
    harness.pulls.then(Ok(snapshot(vec![comment(
        "c1",
        "please add a test",
        false,
    )])));
    bind(&harness, ticket).await;
    let mut targets = harness.bots.follow_targets();
    tokio::time::timeout(
        Duration::from_secs(10),
        targets.wait_for(|targets| targets.iter().any(|target| target.pending)),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        harness.agents.inputs().is_empty(),
        "nothing reaches a busy worker"
    );

    harness.agents.busy.store(false, Ordering::SeqCst);
    tokio::time::pause();
    advance_until(|| !harness.agents.inputs().is_empty()).await;

    assert_eq!(harness.agents.inputs().len(), 1);
    advance_until(|| {
        harness
            .bots
            .follow_targets()
            .borrow()
            .iter()
            .all(|target| !target.pending)
    })
    .await;
    harness.bots.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_input_the_provider_took_before_a_crash_is_not_delivered_twice() {
    let fixture = Fixture::new();
    let first_agents = Agents::default();
    first_agents.lose_reply.store(true, Ordering::SeqCst);
    let receipts = first_agents.receipts.clone();
    let (first, ticket) = {
        let (harness, ticket) = harness(&fixture).await;
        harness.bots.shutdown().await.unwrap();
        (attach_with(fixture.bots(), first_agents), ticket)
    };
    first.pulls.then(Ok(snapshot(Vec::new())));
    first.pulls.then(Ok(snapshot(vec![comment(
        "c1",
        "please add a test",
        false,
    )])));
    bind(&first, ticket).await;
    eventually(|| !first.agents.inputs().is_empty()).await;
    let mut pending = first.bots.follow_targets();
    tokio::time::timeout(
        Duration::from_secs(10),
        pending.wait_for(|targets| targets.iter().any(|target| target.pending)),
    )
    .await
    .unwrap()
    .unwrap();
    first.bots.shutdown().await.unwrap();

    // The provider's durable receipts survive; the restarted Bot sends the same message id.
    let second_agents = Agents {
        receipts,
        ..Agents::default()
    };
    let second = attach_with(fixture.bots(), second_agents);
    let mut settled = second.bots.follow_targets();
    tokio::time::timeout(
        Duration::from_secs(10),
        settled.wait_for(|targets| targets.iter().all(|target| !target.pending)),
    )
    .await
    .unwrap()
    .unwrap();

    assert_eq!(first.agents.inputs().len(), 1);
    assert!(
        second.agents.inputs().is_empty(),
        "the provider deduplicated the repeated message id"
    );
    second.bots.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_answer_to_a_paused_bots_ticket_is_delivered_after_resume() {
    let fixture = Fixture::new();
    let (harness, ticket) = harness(&fixture).await;
    let mut blocked = report(ticket, WorkerReportKind::Blocked);
    blocked.question = Some("Which region?".into());
    harness.bots.report_progress(blocked).await.unwrap();
    harness
        .bots
        .execute("bot.pause.request", json!({"workspaceId":WORKSPACE}))
        .await
        .unwrap();

    let answered = harness
        .bots
        .execute(
            "bot.ticket.answer.request",
            json!({"workspaceId":WORKSPACE,"ticketId":ticket,"agentId":"agent-1",
                "answer":"eu-west-1"}),
        )
        .await
        .unwrap();

    assert_eq!(answered["delivered"], false);
    assert!(harness.agents.inputs().is_empty());
    harness
        .bots
        .execute("bot.resume.request", json!({"workspaceId":WORKSPACE}))
        .await
        .unwrap();
    eventually(|| !harness.agents.inputs().is_empty()).await;
    assert!(harness.agents.inputs()[0].2.contains("Answer: eu-west-1"));
    harness.bots.shutdown().await.unwrap();
}

#[test]
fn the_read_budget_is_shared_and_frees_up_after_its_window() {
    let mut budget = Budget::default();
    let start = tokio::time::Instant::now();

    for _ in 0..READ_BUDGET {
        assert_eq!(budget.spend(start), Ok(()));
    }

    assert_eq!(budget.spend(start), Err(start + READ_WINDOW));
    assert_eq!(budget.spend(start + READ_WINDOW), Ok(()));
}

#[tokio::test]
async fn an_organization_members_comment_waits_for_their_write_access_and_arrives_once() {
    let fixture = Fixture::new();
    let (harness, ticket) = harness(&fixture).await;
    let mut member = comment("c1", "please split this function", false);
    member.access = AuthorAccess::Unconfirmed;
    member.author = "maria".into();
    harness.pulls.then(Ok(snapshot(Vec::new())));
    harness.pulls.then(Ok(snapshot(vec![member])));
    bind(&harness, ticket).await;
    tokio::time::pause();
    advance_until(|| harness.pulls.lookups.load(Ordering::SeqCst) >= 1).await;
    assert!(
        harness.agents.inputs().is_empty(),
        "held while the lookup fails"
    );

    harness.pulls.writers.lock().unwrap().push_back(true);
    advance_until(|| !harness.agents.inputs().is_empty()).await;
    let reads = harness.pulls.reads.load(Ordering::SeqCst);
    advance_until(|| harness.pulls.reads.load(Ordering::SeqCst) >= reads + 2).await;

    assert_eq!(harness.agents.inputs().len(), 1);
    assert_eq!(
        harness.pulls.lookups.load(Ordering::SeqCst),
        2,
        "the confirmed access is kept on the binding"
    );
    harness.bots.shutdown().await.unwrap();
}

/// Re-review 2, L1: eight refusals within about two minutes, such as during a provider
/// restart, used to send the input to the Bot. The run of failures must now last ten minutes
/// of wall-clock time, which the state tests drive with explicit times.
#[tokio::test]
async fn an_input_a_live_agent_refuses_for_a_while_stays_pending() {
    let fixture = Fixture::new();
    let (harness, ticket) = harness(&fixture).await;
    harness.agents.refuse.store(true, Ordering::SeqCst);
    harness.pulls.then(Ok(snapshot(Vec::new())));
    harness.pulls.then(Ok(snapshot(vec![comment(
        "c1",
        "please add a test",
        false,
    )])));
    bind(&harness, ticket).await;
    tokio::time::pause();

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while harness.agents.attempts.load(Ordering::SeqCst) < 12 {
        assert!(std::time::Instant::now() < deadline, "retried in time");
        tokio::time::advance(PENDING_INTERVAL + Duration::from_secs(1)).await;
        std::thread::sleep(Duration::from_millis(2));
        tokio::task::yield_now().await;
    }

    assert_eq!(pull_notifications(&harness.bots).await, 0);
    assert!(
        !ticket_view(&harness.bots, ticket).await["reports"]
            .to_string()
            .contains("went to the Bot")
    );
    harness.agents.refuse.store(false, Ordering::SeqCst);
    advance_until(|| !harness.agents.inputs().is_empty()).await;
    assert_eq!(harness.agents.inputs().len(), 1);
    harness.bots.shutdown().await.unwrap();
}

/// Re-review 2, R1: a lookup that kept failing held the entry silently on every read. Now
/// the author is asked three times, then the Bot hears of the entry as outside activity, and
/// the author is not asked again.
#[tokio::test]
async fn an_organization_member_whose_lookup_keeps_failing_reaches_the_bot_after_three_reads() {
    let fixture = Fixture::new();
    let (harness, ticket) = harness(&fixture).await;
    let mut member = comment("c1", "please split this function", false);
    member.access = AuthorAccess::Unconfirmed;
    member.author = "maria".into();
    harness.pulls.then(Ok(snapshot(Vec::new())));
    harness.pulls.then(Ok(snapshot(vec![member])));
    bind(&harness, ticket).await;
    tokio::time::pause();

    advance_until(|| harness.pulls.lookups.load(Ordering::SeqCst) >= 3).await;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while pull_notifications(&harness.bots).await == 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "the Bot heard of it in time"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let reads = harness.pulls.reads.load(Ordering::SeqCst);
    advance_until(|| harness.pulls.reads.load(Ordering::SeqCst) >= reads + 3).await;

    assert_eq!(harness.pulls.lookups.load(Ordering::SeqCst), 3);
    assert!(
        harness.agents.inputs().is_empty(),
        "never sent to the worker"
    );
    assert_eq!(pull_notifications(&harness.bots).await, 1);
    harness.bots.shutdown().await.unwrap();
}

/// A lookup the read budget leaves no room for counts as a failed attempt, so a saturated
/// budget cannot hold an author without end.
#[tokio::test]
async fn a_lookup_without_read_budget_counts_as_a_failed_attempt() {
    let pulls = Arc::new(Pulls::default());
    let ports = super::Ports {
        pulls: pulls.clone(),
        workers: Arc::new(Agents::default()),
        delivering: Mutex::default(),
    };
    let mut budget = Budget::default();
    let now = tokio::time::Instant::now();
    for _ in 0..READ_BUDGET {
        budget.spend(now).unwrap();
    }

    let lookups = super::lookups(
        &ports,
        WORKSPACE,
        BTreeSet::from(["maria".to_owned()]),
        &mut budget,
    )
    .await;

    assert_eq!(lookups.failed, BTreeSet::from(["maria".to_owned()]));
    assert!(lookups.answered.is_empty());
    assert_eq!(pulls.lookups.load(Ordering::SeqCst), 0);
}

/// Re-review 2, L3: a pull request that can no longer be read was polled without end and
/// pinned its ticket. After eight reads in a row that find no such pull request, the ticket
/// stops following it and the open ticket's Bot hears why.
#[tokio::test]
async fn a_pull_request_that_stays_missing_stops_being_followed() {
    let fixture = Fixture::new();
    let (harness, ticket) = harness(&fixture).await;
    harness.pulls.then(Ok(snapshot(Vec::new())));
    for _ in 0..8 {
        harness.pulls.then(Err(PullReadError::NotFound));
    }
    bind(&harness, ticket).await;
    tokio::time::pause();

    advance_until(|| harness.pulls.reads.load(Ordering::SeqCst) >= 9).await;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while pull_notifications(&harness.bots).await == 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "the Bot heard of it in time"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let view = ticket_view(&harness.bots, ticket).await;
    assert!(view["pulls"].as_array().is_none_or(Vec::is_empty));
    let reads = harness.pulls.reads.load(Ordering::SeqCst);
    for _ in 0..30 {
        tokio::time::advance(BACKOFF_LIMIT).await;
        std::thread::sleep(Duration::from_millis(2));
        tokio::task::yield_now().await;
    }

    assert_eq!(
        harness.pulls.reads.load(Ordering::SeqCst),
        reads,
        "no longer read"
    );
    harness.bots.shutdown().await.unwrap();
}

/// Verify 3, V4: one daemon-wide lock made an answer wait behind sends to other tickets'
/// agents. Deliveries of one ticket share a lock; other tickets get their own, and a lock
/// nobody holds any more is dropped.
#[test]
fn delivery_locks_are_per_ticket_and_dropped_once_unused() {
    let ports = super::Ports {
        pulls: Arc::new(Pulls::default()),
        workers: Arc::new(Agents::default()),
        delivering: Mutex::default(),
    };
    let (first, second) = (TicketId::generate(), TicketId::generate());

    let held = ports.delivery_lock(first);
    assert!(Arc::ptr_eq(&held, &ports.delivery_lock(first)));
    let other = ports.delivery_lock(second);
    assert!(!Arc::ptr_eq(&held, &other));
    assert!(held.try_lock().is_ok() && other.try_lock().is_ok());
    drop(other);
    let _ = ports.delivery_lock(first);

    assert_eq!(
        ports
            .delivering
            .lock()
            .unwrap()
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        vec![first],
        "the unused lock of the other ticket is gone"
    );
}
