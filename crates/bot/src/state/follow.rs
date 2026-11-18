//! Tickets that follow pull requests (ADR-287): what the follower serves, applying its reads,
//! the inputs waiting for a ticket's Agents, and the owner's answers to blocked workers.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use domain::bot::BotState;
use domain::bot::follow::Delivery;
use domain::bot::notification::Notification;
use domain::bot::ticket::{PullNotice, PullObservation, ReportSource, Ticket};
use domain::forge::PullSnapshot;
use domain::{BotId, TicketId};
use model::ErrorCode;
use serde_json::{Value, json};

use super::State;
use super::tickets::{OPEN_LIMIT, full_view, refused};
use crate::follow::{FOLLOW_LIMIT, FollowTarget, FollowedPull, PullRead};
use crate::protocol::TicketAnswer;

impl State {
    /// The tickets of active Bots the follower serves at `now`: those that follow pull
    /// requests, and those with inputs waiting for delivery.
    pub(crate) fn follow_targets(&self, now: DateTime<Utc>) -> Vec<FollowTarget> {
        self.tickets
            .values()
            .filter(|ticket| !ticket.pulls.is_empty() || !ticket.pending.is_empty())
            .filter_map(|ticket| {
                let bot = self.bot(ticket.bot_id).ok()?;
                if !matches!(bot.state, BotState::Active) {
                    return None;
                }
                let pulls = if ticket.follows_pulls(now) {
                    ticket
                        .pulls
                        .iter()
                        .map(|pull| FollowedPull {
                            number: pull.number,
                            mark: pull.mark.as_ref().map(|mark| mark.at),
                            failures: pull.failures,
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                let pending = !ticket.pending.is_empty();
                (!pulls.is_empty() || pending).then(|| FollowTarget {
                    root: bot.workspace_id.clone(),
                    ticket: ticket.id,
                    pulls,
                    ended: ticket.pulls_ended(),
                    pending,
                })
            })
            .collect()
    }

    /// Refuse following pull request `number` on `bot`'s ticket `ticket` at `now` when the Bot
    /// already follows [`FOLLOW_LIMIT`] other pull requests; following it again is fine.
    pub(super) fn check_follow_limit(
        &self,
        bot: BotId,
        ticket: TicketId,
        number: u64,
        now: DateTime<Utc>,
    ) -> Result<(), ErrorCode> {
        let own = self.tickets.get(&ticket).map_or(0, |indexed| {
            indexed
                .pulls
                .iter()
                .filter(|pull| pull.number != number)
                .count()
        });
        if self.followed(bot, ticket, now) + own >= FOLLOW_LIMIT {
            return Err(ErrorCode::ResourceExhausted);
        }
        Ok(())
    }

    /// Refuse reopening `bot`'s closed ticket `ticket` at `now` when that would make the Bot
    /// follow more than [`FOLLOW_LIMIT`] pull requests again; a ticket still followed while
    /// closed already counts.
    pub(super) fn check_reopen_follow_limit(
        &self,
        bot: BotId,
        ticket: &Ticket,
        now: DateTime<Utc>,
    ) -> Result<(), ErrorCode> {
        if ticket.follows_pulls(now) || ticket.pulls.is_empty() {
            return Ok(());
        }
        if self.followed(bot, ticket.id, now) + ticket.pulls.len() > FOLLOW_LIMIT {
            return Err(ErrorCode::ResourceExhausted);
        }
        Ok(())
    }

    /// How many pull requests `bot` follows at `now` on tickets other than `except`.
    fn followed(&self, bot: BotId, except: TicketId, now: DateTime<Utc>) -> usize {
        self.tickets
            .values()
            .filter(|indexed| {
                indexed.bot_id == bot && indexed.id != except && indexed.follows_pulls(now)
            })
            .map(|indexed| indexed.pulls.len())
            .sum()
    }

    /// Apply one read of a followed pull request at `now`: hand inputs for Agents that are no
    /// longer live to the Bot, route what the read found, offer the Bot's notifications before
    /// saving the ticket, and return the inputs to deliver now.
    pub(crate) fn observe_pull(
        &mut self,
        read: &PullRead,
        now: DateTime<Utc>,
    ) -> Result<Vec<Delivery>, ErrorCode> {
        let bot = self.ticket_bot(&read.root)?;
        let mut ticket = self.load_ticket(bot, read.ticket)?;
        let expired = ticket.expire_follow(now);
        let mut notices = ticket.reroute_pending(&read.live, now);
        let observation = PullObservation {
            snapshot: &read.snapshot,
            authored: &read.authored,
            lookups: &read.lookups,
            live: &read.live,
            reopen: self.open_tickets(bot) < OPEN_LIMIT,
            at: now,
        };
        let followed = ticket
            .observe_pull(read.snapshot.number, observation)
            .unwrap_or_default();
        let changed = expired || followed.changed || !notices.is_empty();
        notices.extend(followed.notices);
        self.offer_notices(bot, &ticket, notices, now)?;
        let deliveries = ticket.pending.clone();
        if changed {
            self.save_ticket(ticket)?;
            if expired || followed.closed {
                self.evict_closed(bot, now)?;
            }
        }
        Ok(deliveries)
    }

    /// The authors of `snapshot`, a read of a pull request `root`'s ticket `id` follows, whose
    /// write access the binding needs looked up (ADR-287); none when it no longer follows it.
    pub(crate) fn authors_to_confirm(
        &self,
        root: &str,
        id: TicketId,
        snapshot: &PullSnapshot,
    ) -> Result<BTreeSet<String>, ErrorCode> {
        let bot = self.ticket_bot(root)?;
        let ticket = self.load_ticket(bot, id)?;
        Ok(ticket
            .pulls
            .iter()
            .find(|pull| pull.number == snapshot.number)
            .map(|pull| pull.authors_to_confirm(snapshot))
            .unwrap_or_default())
    }

    /// Record that reading pull request `number` of `root`'s ticket `id` failed with `error`
    /// at `now`, `missing` when the forge found no such pull request. The ticket itself is
    /// unaffected, except that one resumed after its pull requests ended still closes when its
    /// follow window passed, and one whose pull request stayed missing stops following it,
    /// telling the Bot if the ticket is not closed.
    pub(crate) fn pull_failed(
        &mut self,
        root: &str,
        id: TicketId,
        number: u64,
        error: &str,
        missing: bool,
        now: DateTime<Utc>,
    ) -> Result<(), ErrorCode> {
        let bot = self.ticket_bot(root)?;
        let mut ticket = self.load_ticket(bot, id)?;
        let expired = ticket.expire_follow(now);
        let failure = ticket.pull_failed(number, error, missing, now);
        if failure.is_none() && !expired {
            return Ok(());
        }
        let dropped = failure.as_ref().is_some_and(|failure| failure.dropped);
        if let Some(failure) = failure {
            self.offer_notices(bot, &ticket, failure.notices, now)?;
        }
        self.save_ticket(ticket)?;
        if expired || dropped {
            self.evict_closed(bot, now)?;
        }
        Ok(())
    }

    /// The inputs waiting on `root`'s ticket `id` for the `live` Agents, after handing those
    /// for other Agents to the Bot at `now`.
    pub(crate) fn settle_pending(
        &mut self,
        root: &str,
        id: TicketId,
        live: &[String],
        now: DateTime<Utc>,
    ) -> Result<Vec<Delivery>, ErrorCode> {
        let bot = self.ticket_bot(root)?;
        if self.indexed_ticket(bot, id)?.pending.is_empty() {
            return Ok(Vec::new());
        }
        let mut ticket = self.load_ticket(bot, id)?;
        let notices = ticket.reroute_pending(live, now);
        let deliveries = ticket.pending.clone();
        if !notices.is_empty() {
            self.offer_notices(bot, &ticket, notices, now)?;
            self.save_ticket(ticket)?;
        }
        Ok(deliveries)
    }

    /// Forget the inputs `ids` of `root`'s ticket `id`, sent from `sent` on, which their idle
    /// Agents took as new turns by `now`.
    pub(crate) fn delivered(
        &mut self,
        root: &str,
        id: TicketId,
        ids: &[String],
        sent: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<(), ErrorCode> {
        let bot = self.ticket_bot(root)?;
        let mut ticket = self.load_ticket(bot, id)?;
        if ticket.delivered(ids, sent, now) {
            self.save_ticket(ticket)?;
        }
        Ok(())
    }

    /// Count at `now` a failed delivery against each input `failed` of `root`'s ticket `id`,
    /// handing those that failed for too long to the Bot, and start a new run of failures for
    /// each input `busy`, whose Agent answered it is busy.
    pub(crate) fn undelivered(
        &mut self,
        root: &str,
        id: TicketId,
        failed: &[String],
        busy: &[String],
        now: DateTime<Utc>,
    ) -> Result<(), ErrorCode> {
        let bot = self.ticket_bot(root)?;
        let mut ticket = self.load_ticket(bot, id)?;
        let reset = ticket.delivery_busy(busy);
        let notices = ticket.delivery_failed(failed, now);
        let counted = notices.is_some();
        if let Some(notices) = notices {
            self.offer_notices(bot, &ticket, notices, now)?;
        }
        if reset || counted {
            self.save_ticket(ticket)?;
        }
        Ok(())
    }

    /// `bot.ticket.answer.request`: the owner answers the blocked worker the request names,
    /// which the ticket must still wait on, also while the Bot is paused; the answer waits for
    /// delivery to that worker until the Bot is active.
    pub(crate) fn answer_ticket(
        &mut self,
        request: &TicketAnswer,
        now: DateTime<Utc>,
    ) -> Result<Value, ErrorCode> {
        let bot = self.current(&request.workspace_id)?.id;
        let mut ticket = self.load_ticket(bot, request.ticket_id)?;
        ticket
            .answer(ReportSource::User, &request.agent_id, &request.answer, now)
            .map_err(refused)?;
        let view = full_view(&ticket);
        self.save_ticket(ticket)?;
        Ok(json!({"ticket":view}))
    }

    /// Offer `bot`'s notifications about followed pull requests of `ticket` at `now`; the
    /// caller saves the ticket only after they are logged, so a crash in between yields the
    /// same notifications again, which the log drops as duplicates.
    fn offer_notices(
        &mut self,
        bot: BotId,
        ticket: &Ticket,
        notices: Vec<PullNotice>,
        now: DateTime<Utc>,
    ) -> Result<(), ErrorCode> {
        let scope = self.bot(bot)?.workspace_id.clone();
        for notice in notices {
            self.notify(
                Notification {
                    id: notice.id,
                    workspace_id: scope.clone(),
                    scope: Some(scope.clone()),
                    kind: notice.kind,
                    agent_id: notice.agent_id,
                    schedule_id: None,
                    run_id: None,
                    terminal_id: None,
                    title: notice.title,
                    summary: notice.summary,
                    at: now,
                    conversation_id: ticket.conversation_id,
                    ticket_id: Some(ticket.id),
                    ticket_title: Some(ticket.title.clone()),
                    turn_started_at: None,
                },
                now,
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
