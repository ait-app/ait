//! A crash between any two writes of an interrupted wake turn's retry neither loses nor repeats
//! the retry (ADR-271 §11).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use domain::bot::notification::NotificationLog;
use domain::bot::ticket::Ticket;
use domain::bot::{Catalog, Conversation, Turn};
use domain::{BotId, BotTurnId, ConversationId, TicketId};
use model::bot::{StorageError, Store};
use persistence::storage::bot::FileStore;

use super::*;

/// The fixture's store for a process that dies after `writes` more writes: every later write
/// fails, which freezes the state as an uncertain write does.
#[derive(Debug)]
struct Crash {
    inner: Arc<FileStore>,
    writes: AtomicUsize,
}

impl Crash {
    fn after(fixture: &Fixture, writes: usize) -> Arc<Self> {
        Arc::new(Self {
            inner: fixture.store.clone(),
            writes: AtomicUsize::new(writes),
        })
    }

    fn write(&self) -> Result<(), StorageError> {
        self.writes
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                left.checked_sub(1)
            })
            .map(|_| ())
            .map_err(|_| StorageError)
    }

    /// Whether the process would have died before finishing.
    fn crashed(&self) -> bool {
        self.writes.load(Ordering::SeqCst) == 0
    }
}

impl Store for Crash {
    fn catalog(&self) -> Result<Catalog, StorageError> {
        self.inner.catalog()
    }
    fn save_catalog(&self, catalog: &Catalog) -> Result<(), StorageError> {
        self.write()?;
        self.inner.save_catalog(catalog)
    }
    fn conversations(&self) -> Result<Vec<Conversation>, StorageError> {
        self.inner.conversations()
    }
    fn save_conversation(&self, conversation: &Conversation) -> Result<(), StorageError> {
        self.write()?;
        self.inner.save_conversation(conversation)
    }
    fn delete_conversation(&self, id: ConversationId) -> Result<(), StorageError> {
        self.write()?;
        self.inner.delete_conversation(id)
    }
    fn turns(&self, id: ConversationId) -> Result<Vec<Turn>, StorageError> {
        self.inner.turns(id)
    }
    fn scan_turns(
        &self,
        id: ConversationId,
        visitor: &mut dyn FnMut(Turn) -> Result<(), StorageError>,
    ) -> Result<(), StorageError> {
        self.inner.scan_turns(id, visitor)
    }
    fn turn(&self, conversation: ConversationId, id: BotTurnId) -> Result<Turn, StorageError> {
        self.inner.turn(conversation, id)
    }
    fn save_turn(&self, turn: &Turn) -> Result<(), StorageError> {
        self.write()?;
        self.inner.save_turn(turn)
    }
    fn memory(&self, workspace: &str) -> Result<String, StorageError> {
        self.inner.memory(workspace)
    }
    fn save_memory(&self, workspace: &str, text: &str) -> Result<(), StorageError> {
        self.write()?;
        self.inner.save_memory(workspace, text)
    }
    fn delete_memory(&self, workspace: &str) -> Result<(), StorageError> {
        self.write()?;
        self.inner.delete_memory(workspace)
    }
    fn notification_logs(&self) -> Result<Vec<NotificationLog>, StorageError> {
        self.inner.notification_logs()
    }
    fn save_notification_log(&self, log: &NotificationLog) -> Result<(), StorageError> {
        self.write()?;
        self.inner.save_notification_log(log)
    }
    fn delete_notification_log(&self, bot: BotId) -> Result<(), StorageError> {
        self.write()?;
        self.inner.delete_notification_log(bot)
    }
    fn scan_tickets(
        &self,
        visitor: &mut dyn FnMut(Ticket) -> Result<(), StorageError>,
    ) -> Result<(), StorageError> {
        self.inner.scan_tickets(visitor)
    }
    fn ticket(&self, bot: BotId, id: TicketId) -> Result<Ticket, StorageError> {
        self.inner.ticket(bot, id)
    }
    fn save_ticket(&self, ticket: &Ticket) -> Result<(), StorageError> {
        self.write()?;
        self.inner.save_ticket(ticket)
    }
    fn delete_ticket(&self, bot: BotId, id: TicketId) -> Result<(), StorageError> {
        self.write()?;
        self.inner.delete_ticket(bot, id)
    }
    fn delete_tickets(&self, bot: BotId) -> Result<(), StorageError> {
        self.write()?;
        self.inner.delete_tickets(bot)
    }
}

/// A fixture whose wake turn for notification `a`, raised in a Telegram conversation, runs.
fn running_wake() -> (Fixture, State, BotTurnId) {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let channel = telegram(&mut state);
    let origin = telegram_conversation(&mut state, channel);
    let turn = start_wake(&mut state, from("a", origin));
    (fixture, state, turn)
}

/// Restart on the intact store and assert that `a` woke exactly one retry turn.
fn assert_retried_once(fixture: &Fixture, writes: usize) {
    let mut restarted = fixture.state();
    restarted.wake(later(10));
    restarted.wake(later(60));

    let turns = wake_turns(fixture);
    assert_eq!(turns.len(), 2, "crash after {writes} writes");
    assert_eq!(
        wake_state(&restarted, "a")["turnId"],
        json!(turns[1].id),
        "crash after {writes} writes"
    );
}

#[test]
fn a_crash_while_shutdown_cancels_a_wake_turn_still_retries_it_once() {
    for writes in 0.. {
        let (fixture, mut state, turn) = running_wake();
        let crash = Crash::after(&fixture, writes);
        state.store = crash.clone();
        state.stopping = true;

        // An error here is the crash itself; the state is frozen and dropped.
        let _ = state.progress(turn, TurnStatus::Cancelled, None, None);
        drop(state);

        assert_retried_once(&fixture, writes);
        if !crash.crashed() {
            break;
        }
    }
}

#[test]
fn a_crash_while_start_up_requeues_an_interrupted_wake_turn_still_retries_it_once() {
    for writes in 0.. {
        let (fixture, state, _) = running_wake();
        // The daemon died while the wake turn ran.
        drop(state);
        let crash = Crash::after(&fixture, writes);

        let loaded = State::load(
            crash.clone(),
            fixture.workspaces.clone(),
            fixture.projects.clone(),
            fixture.scopes.clone(),
        );
        drop(loaded);

        assert_retried_once(&fixture, writes);
        if !crash.crashed() {
            break;
        }
    }
}

/// A ticket of the active Bot with `agent-1` assigned, asked in the owner conversation.
fn ticketed(state: &mut State) -> TicketId {
    let conversation = owner(state);
    let ticket = state
        .open_ticket(
            crate::tickets::OpenTicket {
                root: WORKSPACE.into(),
                draft: domain::bot::ticket::TicketDraft {
                    conversation_id: Some(conversation),
                    key: "k".into(),
                    title: "Fix login".into(),
                    request: "Fix the login crash".into(),
                },
            },
            Utc::now(),
        )
        .unwrap()
        .ticket_id;
    state
        .assign_ticket(
            &crate::tickets::AssignTicket {
                root: WORKSPACE.into(),
                ticket_id: ticket,
                agent_id: "agent-1".into(),
                workspace_id: WORKSPACE.into(),
            },
            Utc::now(),
        )
        .unwrap();
    ticket
}

fn done(ticket: TicketId) -> crate::tickets::WorkerReport {
    crate::tickets::WorkerReport {
        root: WORKSPACE.into(),
        ticket_id: ticket,
        agent_id: "agent-1".into(),
        workspace_id: WORKSPACE.into(),
        kind: domain::bot::ticket::WorkerReportKind::Done,
        text: "merged".into(),
    }
}

#[test]
fn a_crash_after_a_worker_report_still_wakes_the_bot_exactly_once() {
    for writes in 0.. {
        let fixture = Fixture::new();
        let mut state = active(&fixture);
        let ticket = ticketed(&mut state);
        let crash = Crash::after(&fixture, writes);
        state.store = crash.clone();

        // An error here is the crash itself; the state is frozen and dropped. The first write
        // saves the ticket, so any write at all records the report.
        let _ = state.report_progress(&done(ticket), Utc::now());
        let recorded = writes > 0;
        drop(state);
        let mut restarted = fixture.state();
        restarted.wake(later(60));
        drop(restarted);
        let mut again = fixture.state();
        again.wake(later(120));

        let woke: Vec<String> = wake_turns(&fixture)
            .iter()
            .flat_map(|turn| &turn.wake.as_ref().unwrap().notifications)
            .map(|notification| notification.id.clone())
            .collect();
        let expected = if recorded {
            vec![format!("report:{ticket}:1")]
        } else {
            Vec::new()
        };
        assert_eq!(woke, expected, "crash after {writes} writes");
        if !crash.crashed() {
            break;
        }
    }
}

#[test]
fn reports_older_than_the_wake_window_are_not_offered_again() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let ticket = ticketed(&mut state);
    let crash = Crash::after(&fixture, 1);
    state.store = crash;
    let _ = state.report_progress(&done(ticket), later(-2 * 3600));
    drop(state);

    let mut restarted = fixture.state();
    restarted.wake(later(60));

    assert!(wake_turns(&fixture).is_empty());
    assert!(
        fixture
            .store
            .notification_logs()
            .unwrap()
            .iter()
            .all(|log| log.entries.is_empty())
    );
}
