//! Workspace notifications: admission into each Bot's log and inbox, coalesced wake turns, and
//! the overview feed (ADR-271).

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use chrono::{DateTime, TimeDelta, Utc};
use domain::bot::notification::{
    LoggedNotification, Notification, NotificationKind, NotificationLog, SkipReason, Wake,
    WakeStatus,
};
use domain::bot::{
    Bot, BotState, Catalog, ChannelConfig, Conversation, Turn, TurnStatus, WakeScope,
};
use domain::{BotId, BotTurnId, ConversationId};
use model::ErrorCode;
use model::bot::Store;
use serde_json::{Value, json};

use super::State;
use crate::channels::Incoming;
use crate::protocol::NotificationsList;
use crate::service::Notified;

/// Longest notification id in bytes.
const ID_LIMIT: usize = 256;
/// Longest kept notification title in bytes; longer titles are truncated.
const TITLE_LIMIT: usize = 256;
/// Longest kept notification summary in bytes; longer summaries are truncated.
const SUMMARY_LIMIT: usize = 1024;
/// Most pending notifications one Bot keeps; later in-scope ones are logged as skipped.
const INBOX_LIMIT: usize = 64;
/// Most settled (not pending) notifications one Bot's log keeps; the oldest go first.
const LOG_LIMIT: usize = 128;
/// Quiet time after the newest pending notification before a wake turn starts.
const WAKE_QUIET: TimeDelta = TimeDelta::seconds(3);
/// Longest a pending notification waits for quiet before its wake turn starts anyway.
const WAKE_MAX_DELAY: TimeDelta = TimeDelta::seconds(30);
/// Most notifications folded into one wake turn; the rest wait for the next one.
const WAKE_BATCH: usize = 16;
/// Most wake turns per conversation within [`WAKE_WINDOW`].
const WAKE_BUDGET: usize = 12;
/// Rolling window of [`WAKE_BUDGET`].
const WAKE_WINDOW: TimeDelta = TimeDelta::hours(1);
/// Most Agent observations kept across all conversations; the oldest are forgotten first.
const OBSERVATION_LIMIT: usize = 256;
/// Default and largest page of `bot.notifications.list.request`.
const LIST_DEFAULT: usize = 50;
const LIST_LIMIT: usize = 128;

/// Reads of settled Agents by a conversation's running turn, pending its outcome.
#[derive(Debug)]
pub(super) struct Observing {
    /// When the turn started executing; a read whose call began earlier was another turn's.
    started: Instant,
    /// The latest time each Agent the turn read had last changed.
    agents: BTreeMap<String, DateTime<Utc>>,
}

/// A wake turn found on disk while loading, used to reconcile its Bot's inbox.
pub(super) struct StoredWake {
    bot: BotId,
    conversation: ConversationId,
    turn: BotTurnId,
    ids: BTreeSet<String>,
    /// Still queued: a crash may have lost the log save that marked its entries woken.
    queued: bool,
    /// Found executing: the daemon stopped without settling it, so its entries are retried.
    interrupted: bool,
}

impl State {
    /// Offer `notification` to the active Bots of its Workspace and scope root, logging it and
    /// queueing it for a wake turn where it is in the Bot's wake scope.
    pub(crate) fn notify(
        &mut self,
        notification: Notification,
        now: DateTime<Utc>,
    ) -> Result<Notified, ErrorCode> {
        self.ensure_writable()?;
        let notification = bounded(notification)?;
        let mut bots: Vec<(BotId, bool)> = Vec::with_capacity(2);
        let roots = std::iter::once(notification.workspace_id.as_str())
            .chain(notification.scope.as_deref());
        for root in roots {
            if let Ok(bot) = self.current(root)
                && matches!(bot.state, BotState::Active)
                && bots.iter().all(|(id, _)| *id != bot.id)
            {
                bots.push((bot.id, wakes(bot, &notification)));
            }
        }
        if bots.is_empty() {
            return Ok(Notified::Ignored);
        }
        let mut outcome = Notified::Duplicate;
        for (bot, in_scope) in bots {
            if self.log_notification(bot, in_scope, &notification, now)? {
                outcome = Notified::Admitted;
            }
        }
        Ok(outcome)
    }

    /// Append `notification` to `bot`'s log; `false` when the log already holds its id.
    fn log_notification(
        &mut self,
        bot: BotId,
        in_scope: bool,
        notification: &Notification,
        now: DateTime<Utc>,
    ) -> Result<bool, ErrorCode> {
        // A replay of a notification that already woke the Bot, evicted from the log since.
        if self
            .woken
            .get(&(bot, notification.id.clone()))
            .is_some_and(|woke| now - *woke < WAKE_WINDOW)
        {
            return Ok(false);
        }
        let log = self
            .notifications
            .entry(bot)
            .or_insert_with(|| NotificationLog::new(bot));
        if log
            .entries
            .iter()
            .any(|entry| entry.notification.id == notification.id)
        {
            return Ok(false);
        }
        let sequence = log.next_sequence;
        let next_sequence = sequence
            .checked_add(1)
            .ok_or(ErrorCode::ResourceExhausted)?;
        let wake = if !in_scope {
            WakeStatus::Logged
        } else if pending(log).count() >= INBOX_LIMIT {
            WakeStatus::Skipped {
                reason: SkipReason::InboxFull,
            }
        } else {
            WakeStatus::Pending
        };
        log.entries.push(LoggedNotification {
            sequence,
            admitted_at: now,
            notification: notification.clone(),
            wake,
        });
        log.next_sequence = next_sequence;
        evict(&mut log.entries);
        self.save_notification_log(bot)?;
        Ok(true)
    }

    /// Start every wake turn that is due at `now`. Failures leave the notifications pending
    /// for the next call, except an uncertain write, which freezes the state.
    pub(crate) fn wake(&mut self, now: DateTime<Utc>) {
        if self.frozen {
            return;
        }
        let bots: Vec<BotId> = self
            .notifications
            .iter()
            .filter(|(_, log)| pending(log).next().is_some())
            .map(|(bot, _)| *bot)
            .collect();
        for bot in bots {
            if let Err(error) = self.wake_bot(bot, now) {
                if self.frozen {
                    return;
                }
                tracing::debug!(%bot, ?error, "Bot wake-up deferred");
            }
        }
    }

    fn wake_bot(&mut self, bot: BotId, now: DateTime<Utc>) -> Result<(), ErrorCode> {
        let current = self.bot(bot)?;
        if !matches!(current.state, BotState::Active) || self.workspace(current).is_err() {
            return Ok(());
        }
        let builtin = current.builtin_channel_id;
        let owner = self
            .conversations
            .values()
            .find(|conversation| {
                conversation.channel_id == builtin
                    && conversation.peer == "owner"
                    && conversation.sender == "owner"
                    && conversation.thread.is_none()
            })
            .map(|conversation| conversation.id);
        let mut groups: BTreeMap<Option<ConversationId>, Vec<&LoggedNotification>> =
            BTreeMap::new();
        let mut observed = Vec::new();
        let log = self.notifications.get(&bot).ok_or(ErrorCode::BotIo)?;
        for entry in pending(log) {
            let target = entry
                .notification
                .conversation_id
                .filter(|origin| self.wake_target(bot, *origin))
                .or(owner);
            if self.observed(target, &entry.notification) {
                observed.push(entry.sequence);
            } else {
                groups.entry(target).or_default().push(entry);
            }
        }
        let due: Vec<(Option<ConversationId>, Vec<u64>)> = groups
            .into_iter()
            .filter(|(_, entries)| coalesced(entries, now))
            .map(|(target, entries)| {
                let sequences = entries.iter().map(|entry| entry.sequence).collect();
                (target, sequences)
            })
            .collect();
        if !observed.is_empty() {
            let skipped = WakeStatus::Skipped {
                reason: SkipReason::Observed,
            };
            self.mark(bot, &observed, skipped)?;
        }
        for (target, sequences) in due {
            self.wake_conversation(bot, target, &sequences, now)?;
        }
        Ok(())
    }

    /// Whether a completed turn of `target` already read the Agent `notification` is about,
    /// settled no earlier than the notification's event.
    fn observed(&self, target: Option<ConversationId>, notification: &Notification) -> bool {
        let about_agent = matches!(
            notification.kind,
            NotificationKind::AgentFinished
                | NotificationKind::AgentFailed
                | NotificationKind::AgentPermission
        );
        let (Some(target), Some(agent)) = (target, &notification.agent_id) else {
            return false;
        };
        if !about_agent {
            return false;
        }
        self.observations
            .get(&(target, agent.clone()))
            .is_some_and(|seen| notification.at <= *seen)
    }

    /// Fold the pending `sequences` into one wake turn of `target` (the owner conversation,
    /// created on demand, when `None`), or skip them when `target` is blocked by an uncertain
    /// turn or over its budget.
    fn wake_conversation(
        &mut self,
        bot: BotId,
        target: Option<ConversationId>,
        sequences: &[u64],
        now: DateTime<Utc>,
    ) -> Result<(), ErrorCode> {
        if let Some(id) = target {
            // Held entries would fill the inbox and starve every healthy conversation.
            if self.stuck(id) {
                let skipped = WakeStatus::Skipped {
                    reason: SkipReason::Blocked,
                };
                return self.mark(bot, sequences, skipped);
            }
            if self.busy(id) {
                return Ok(());
            }
            if self.wake_budget_used(id, now) >= WAKE_BUDGET {
                let skipped = WakeStatus::Skipped {
                    reason: SkipReason::OverBudget,
                };
                return self.mark(bot, sequences, skipped);
            }
        }
        if self.backlog_full() {
            return Ok(());
        }
        let conversation = match target {
            Some(id) => self.conversations.get(&id).ok_or(ErrorCode::BotIo)?.clone(),
            None => self.route(&owner_input(self.bot(bot)?))?,
        };
        let batch = &sequences[..sequences.len().min(WAKE_BATCH)];
        let log = self.notifications.get(&bot).ok_or(ErrorCode::BotIo)?;
        let notifications: Vec<Notification> = log
            .entries
            .iter()
            .filter(|entry| batch.contains(&entry.sequence))
            .map(|entry| entry.notification.clone())
            .collect();
        let mut turn = super::queued_turn(
            &conversation,
            String::new(),
            crate::instructions::wake_input(&notifications),
            now,
        );
        let id = turn.id;
        turn.message_id = format!("notification:{id}");
        turn.wake = Some(Wake { notifications });
        let conversation_id = conversation.id;
        // The turn is committed first: on restart its notification ids mark the entries woken,
        // so a crash before the log is saved neither loses nor repeats the wake-up.
        self.enqueue(conversation, &turn)?;
        let times = self.wakes.entry(conversation_id).or_default();
        times.retain(|time| now - *time < WAKE_WINDOW);
        times.push(now);
        self.remember_woken(bot, turn.wake.iter(), now);
        let woke = WakeStatus::Woke {
            conversation_id,
            turn_id: id,
        };
        self.mark(bot, batch, woke)
    }

    /// Whether `conversation` of `bot` can still receive its notifications' wake turns.
    fn wake_target(&self, bot: BotId, conversation: ConversationId) -> bool {
        let Some(conversation) = self
            .conversations
            .get(&conversation)
            .filter(|conversation| conversation.bot_id == bot)
        else {
            return false;
        };
        // A webhook channel only answers turns the center started, so it cannot be woken.
        self.channel(bot, conversation.channel_id)
            .is_ok_and(|channel| {
                channel.enabled
                    && !channel.deleted
                    && !matches!(channel.config, ChannelConfig::Webhook { .. })
            })
            && !self.stuck(conversation.id)
    }

    /// Whether `conversation` has running or queued work a wake turn must follow.
    fn busy(&self, conversation: ConversationId) -> bool {
        self.running.contains_key(&conversation)
            || self
                .pending
                .range((conversation, 0)..=(conversation, u64::MAX))
                .next()
                .is_some()
    }

    /// Whether `conversation`'s current generation holds an uncertain turn, so nothing runs
    /// there until the user resets it.
    fn stuck(&self, conversation: ConversationId) -> bool {
        self.conversations
            .get(&conversation)
            .is_some_and(|entry| self.uncertain.contains(&(conversation, entry.generation)))
    }

    /// Remember the notification ids `wakes` carry as having woken `bot` at `at`, forgetting
    /// those older than [`WAKE_WINDOW`], so a replay after the log evicted them stays a duplicate.
    fn remember_woken<'a>(
        &mut self,
        bot: BotId,
        wakes: impl Iterator<Item = &'a Wake>,
        at: DateTime<Utc>,
    ) {
        self.woken.retain(|_, woke| at - *woke < WAKE_WINDOW);
        for notification in wakes.flat_map(|wake| &wake.notifications) {
            let woke = self
                .woken
                .entry((bot, notification.id.clone()))
                .or_insert(at);
            *woke = (*woke).max(at);
        }
    }

    /// Put the notifications wake `turn` carries back in the inbox: the daemon stopped while
    /// it ran, so the user never got its reply.
    pub(super) fn retry_wake(&mut self, turn: &Turn) -> Result<(), ErrorCode> {
        let Some(bot) = self
            .conversations
            .get(&turn.conversation_id)
            .map(|conversation| conversation.bot_id)
        else {
            return Ok(());
        };
        let sequences: Vec<u64> = self
            .notifications
            .get(&bot)
            .into_iter()
            .flat_map(|log| &log.entries)
            .filter(|entry| matches!(entry.wake, WakeStatus::Woke { turn_id, .. } if turn_id == turn.id))
            .map(|entry| entry.sequence)
            .collect();
        if sequences.is_empty() {
            return Ok(());
        }
        self.mark(bot, &sequences, WakeStatus::Pending)
    }

    fn wake_budget_used(&self, conversation: ConversationId, now: DateTime<Utc>) -> usize {
        self.wakes.get(&conversation).map_or(0, |times| {
            times
                .iter()
                .filter(|time| now - **time < WAKE_WINDOW)
                .count()
        })
    }

    /// Set the wake status of `bot`'s entries with the given sequences and save the log.
    fn mark(&mut self, bot: BotId, sequences: &[u64], wake: WakeStatus) -> Result<(), ErrorCode> {
        let log = self.notifications.get_mut(&bot).ok_or(ErrorCode::BotIo)?;
        for entry in &mut log.entries {
            if sequences.contains(&entry.sequence) {
                entry.wake = wake;
            }
        }
        evict(&mut log.entries);
        self.save_notification_log(bot)
    }

    /// Drop `bot`'s pending notifications from its inbox, keeping them in the log.
    pub(super) fn skip_pending(&mut self, bot: BotId, reason: SkipReason) -> Result<(), ErrorCode> {
        let Some(log) = self.notifications.get(&bot) else {
            return Ok(());
        };
        let sequences: Vec<u64> = pending(log).map(|entry| entry.sequence).collect();
        if sequences.is_empty() {
            return Ok(());
        }
        self.mark(bot, &sequences, WakeStatus::Skipped { reason })
    }

    /// Remove the notification state of retired `bot`.
    pub(super) fn delete_notifications(&mut self, bot: BotId) -> Result<(), ErrorCode> {
        self.ensure_writable()?;
        if self.store.delete_notification_log(bot).is_err() {
            self.frozen = true;
            return Err(ErrorCode::BotIo);
        }
        self.notifications.remove(&bot);
        Ok(())
    }

    /// The newest log sequence a turn of `bot` starting now has in front of it.
    pub(super) fn notification_head(&self, bot: BotId) -> Option<u64> {
        self.notifications
            .get(&bot)
            .map(NotificationLog::head)
            .filter(|head| *head > 0)
    }

    /// Count stored wake `turn` toward its conversation's budget and its Bot's dedupe, and
    /// remember its notifications for [`State::reconcile_wakes`]; `interrupted` when the turn
    /// was found executing, which the daemon stopped without settling.
    pub(super) fn load_wake(
        &mut self,
        conversation: &Conversation,
        turn: &Turn,
        interrupted: bool,
        now: DateTime<Utc>,
        stored: &mut Vec<StoredWake>,
    ) {
        let Some(wake) = &turn.wake else {
            return;
        };
        if let Ok(created) = DateTime::parse_from_rfc3339(&turn.created_at) {
            let created = created.with_timezone(&Utc);
            if now - created < WAKE_WINDOW {
                self.wakes.entry(conversation.id).or_default().push(created);
                self.remember_woken(conversation.bot_id, std::iter::once(wake), created);
            }
        }
        stored.push(StoredWake {
            bot: conversation.bot_id,
            conversation: conversation.id,
            turn: turn.id,
            ids: wake
                .notifications
                .iter()
                .map(|notification| notification.id.clone())
                .collect(),
            queued: turn.status == TurnStatus::Queued,
            interrupted,
        });
    }

    /// Mark the notifications a stored queued wake turn carries as woken, so a crash between
    /// committing a wake turn and saving its log never starts a second one, and put those of
    /// interrupted wake turns back in the inbox.
    pub(super) fn reconcile_wakes(&mut self, stored: Vec<StoredWake>) -> Result<(), ErrorCode> {
        let mut changed = BTreeSet::new();
        for wake in stored {
            let Some(log) = self.notifications.get_mut(&wake.bot) else {
                continue;
            };
            let woke = WakeStatus::Woke {
                conversation_id: wake.conversation,
                turn_id: wake.turn,
            };
            for entry in &mut log.entries {
                let next = if wake.interrupted && entry.wake == woke {
                    WakeStatus::Pending
                } else if wake.queued
                    && entry.wake == WakeStatus::Pending
                    && wake.ids.contains(&entry.notification.id)
                {
                    woke
                } else {
                    continue;
                };
                entry.wake = next;
                changed.insert(wake.bot);
            }
        }
        for bot in changed {
            self.save_notification_log(bot)?;
        }
        Ok(())
    }

    /// The Workspace's recent notifications, newest first, optionally only those a
    /// conversation has not had in front of it in a completed turn.
    pub(super) fn list_notifications(
        &self,
        request: &NotificationsList,
    ) -> Result<Value, ErrorCode> {
        let bot = self.current(&request.workspace_id)?;
        let limit = request.limit.unwrap_or(LIST_DEFAULT);
        if limit == 0 || limit > LIST_LIMIT {
            return Err(ErrorCode::InvalidMessage);
        }
        let seen = match request.unseen_by {
            Some(conversation) => {
                self.conversation(bot.id, conversation)?;
                self.seen.get(&conversation).copied()
            }
            None => None,
        };
        let log = self.notifications.get(&bot.id);
        let after = seen.unwrap_or(0);
        let entries: Vec<&LoggedNotification> = log
            .into_iter()
            .flat_map(|log| log.entries.iter().rev())
            .filter(|entry| entry.sequence > after)
            .take(limit)
            .collect();
        Ok(
            json!({"headSequence":log.map_or(0, NotificationLog::head),"seenSequence":seen,
            "wakeScope":bot.wake_scope,"notifications":entries}),
        )
    }

    fn save_notification_log(&mut self, bot: BotId) -> Result<(), ErrorCode> {
        self.ensure_writable()?;
        let log = self.notifications.get(&bot).ok_or(ErrorCode::BotIo)?;
        if self.store.save_notification_log(log).is_err() {
            self.frozen = true;
            return Err(ErrorCode::BotIo);
        }
        Ok(())
    }

    /// Open the observations of the turn of `conversation` that starts executing at `started`.
    pub(super) fn start_observing(&mut self, conversation: ConversationId, started: Instant) {
        self.observing.insert(
            conversation,
            Observing {
                started,
                agents: BTreeMap::new(),
            },
        );
    }

    /// Record that the turn running in `conversation` read `agent` no longer running, in a
    /// state the Agent last changed at `at`, through a call that began at `read_started`. Once
    /// that turn completes, the conversation's notifications about the Agent raised no later
    /// than `at` are skipped as observed instead of waking it; a turn that ends any other way
    /// never told the user, so its observations are dropped. A read outside a running turn, or
    /// one whose call began before the running turn started executing and so was made by an
    /// earlier turn, is ignored.
    ///
    /// Kept in memory only and bounded by [`OBSERVATION_LIMIT`]; after a restart a notification
    /// can wake the conversation once more, which repeats a report but never loses one.
    pub(crate) fn observe(
        &mut self,
        conversation: ConversationId,
        agent: &str,
        at: DateTime<Utc>,
        read_started: Instant,
    ) {
        if let Some(observing) = self.observing.get_mut(&conversation)
            && read_started >= observing.started
        {
            remember_observation(&mut observing.agents, agent.to_owned(), at);
        }
    }

    /// Settle the observations of `conversation`'s turn that just ended: they take effect when
    /// it `completed` and are dropped otherwise.
    pub(super) fn settle_observations(&mut self, conversation: ConversationId, completed: bool) {
        let Some(observing) = self.observing.remove(&conversation) else {
            return;
        };
        if completed {
            for (agent, at) in observing.agents {
                remember_observation(&mut self.observations, (conversation, agent), at);
            }
        }
    }

    /// Record that completed `turn` had the notifications up to its cursor in front of it.
    pub(super) fn saw_notifications(&mut self, turn: &Turn) {
        if let Some(cursor) = turn.notification_cursor {
            let seen = self.seen.entry(turn.conversation_id).or_default();
            *seen = (*seen).max(cursor);
        }
    }
}

/// Load the notification logs of the catalog's live Bots, deleting those a crash after
/// retiring a Bot left behind; nothing else reads them.
pub(super) fn load(
    store: &dyn Store,
    catalog: &Catalog,
) -> Result<BTreeMap<BotId, NotificationLog>, ErrorCode> {
    let mut notifications = BTreeMap::new();
    for log in store.notification_logs().map_err(|_| ErrorCode::BotIo)? {
        let live = catalog
            .bots
            .iter()
            .any(|bot| bot.id == log.bot_id && !matches!(bot.state, BotState::Retired { .. }));
        if !live {
            store
                .delete_notification_log(log.bot_id)
                .map_err(|_| ErrorCode::BotIo)?;
        } else if notifications.insert(log.bot_id, log).is_some() {
            return Err(ErrorCode::BotIo);
        }
    }
    Ok(notifications)
}

/// Raise `key`'s observation time in `observations` to `at`, forgetting the oldest beyond
/// [`OBSERVATION_LIMIT`].
fn remember_observation<K: Ord + Clone>(
    observations: &mut BTreeMap<K, DateTime<Utc>>,
    key: K,
    at: DateTime<Utc>,
) {
    let seen = observations.entry(key).or_insert(at);
    *seen = (*seen).max(at);
    if observations.len() > OBSERVATION_LIMIT
        && let Some(oldest) = observations
            .iter()
            .min_by_key(|(_, seen)| **seen)
            .map(|(key, _)| key.clone())
    {
        observations.remove(&oldest);
    }
}

/// Whether `notification` is inside `bot`'s wake scope.
fn wakes(bot: &Bot, notification: &Notification) -> bool {
    notification.scope.as_deref() == Some(bot.workspace_id.as_str())
        || (bot.wake_scope == WakeScope::All && notification.workspace_id == bot.workspace_id)
}

/// Whether a group of pending entries has been quiet long enough, or waited too long.
fn coalesced(entries: &[&LoggedNotification], now: DateTime<Utc>) -> bool {
    let newest = entries.iter().map(|entry| entry.admitted_at).max();
    let oldest = entries.iter().map(|entry| entry.admitted_at).min();
    newest.is_some_and(|newest| now - newest >= WAKE_QUIET)
        || oldest.is_some_and(|oldest| now - oldest >= WAKE_MAX_DELAY)
}

fn pending(log: &NotificationLog) -> impl Iterator<Item = &LoggedNotification> {
    log.entries
        .iter()
        .filter(|entry| entry.wake == WakeStatus::Pending)
}

/// Drop the oldest settled entries beyond [`LOG_LIMIT`]; pending entries are never dropped.
fn evict(entries: &mut Vec<LoggedNotification>) {
    let settled = entries
        .iter()
        .filter(|entry| entry.wake != WakeStatus::Pending)
        .count();
    let mut excess = settled.saturating_sub(LOG_LIMIT);
    if excess == 0 {
        return;
    }
    entries.retain(|entry| {
        if excess > 0 && entry.wake != WakeStatus::Pending {
            excess -= 1;
            return false;
        }
        true
    });
}

/// The built-in owner route of `bot`, as `bot.conversation.open` opens it.
fn owner_input(bot: &Bot) -> Incoming {
    Incoming {
        channel_id: bot.builtin_channel_id,
        peer: "owner".into(),
        sender: "owner".into(),
        thread: None,
        message_id: String::new(),
        text: String::new(),
        reply_to: None,
    }
}

/// Validate `notification`'s identities and truncate its human text to the stored bounds.
fn bounded(mut notification: Notification) -> Result<Notification, ErrorCode> {
    let id = &notification.id;
    let valid_id = !id.is_empty() && id.len() <= ID_LIMIT && !id.chars().any(char::is_control);
    let ids = [
        &notification.scope,
        &notification.agent_id,
        &notification.schedule_id,
        &notification.run_id,
        &notification.terminal_id,
    ];
    if !valid_id
        || !model::valid_id(&notification.workspace_id)
        || ids.into_iter().flatten().any(|id| !model::valid_id(id))
    {
        return Err(ErrorCode::InvalidMessage);
    }
    truncate(&mut notification.title, TITLE_LIMIT);
    if let Some(summary) = &mut notification.summary {
        truncate(summary, SUMMARY_LIMIT);
    }
    Ok(notification)
}

/// Shorten `text` to at most `limit` bytes on a character boundary.
fn truncate(text: &mut String, limit: usize) {
    if text.len() > limit {
        let end = (0..=limit)
            .rev()
            .find(|index| text.is_char_boundary(*index))
            .unwrap_or(0);
        text.truncate(end);
    }
}

#[cfg(test)]
mod tests;
