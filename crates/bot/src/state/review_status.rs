//! The status board a GitHub review channel keeps on each pull request (ADR-321).
//!
//! The Bot state appends lines as review turns move; the channel worker renders the board into
//! one comment and reports what it wrote. A board that fails to save stays in memory and is
//! retried with the next change, so a status problem never stops a review.

use std::collections::BTreeMap;

use chrono::Utc;
use domain::bot::review_status::{ReviewStatus, ReviewStatusKey, ReviewStatusPage, StatusEvent};
use domain::bot::{ChannelConfig, DeliveryStatus, Turn, TurnContext, TurnStatus};
use domain::{BotTurnId, ChannelId};
use model::ErrorCode;
use model::bot::Store;

use super::State;

/// What the channel worker wrote for one board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StatusWritten {
    /// The board.
    pub(crate) key: ReviewStatusKey,
    /// Revision the written comment shows.
    pub(crate) revision: u64,
    /// GitHub comments holding the board.
    pub(crate) pages: Vec<ReviewStatusPage>,
    /// Trigger comments that received their reaction.
    pub(crate) reacted: Vec<u64>,
}

/// Every stored board of a channel that is not deleted.
pub(super) fn load(
    store: &dyn Store,
    deleted: &[ChannelId],
) -> BTreeMap<ReviewStatusKey, ReviewStatus> {
    for channel in deleted {
        // Retried on every start, like a deleted channel's secret.
        let _ = store.delete_review_statuses(*channel);
    }
    store
        .review_statuses()
        .unwrap_or_default()
        .into_iter()
        .filter(|status| !deleted.contains(&status.key.channel_id))
        .map(|status| (status.key.clone(), status))
        .collect()
}

impl State {
    /// Record what a change of `turn` from `previous` means for its pull request's board.
    pub(super) fn review_transition(&mut self, previous: Option<TurnStatus>, turn: &Turn) {
        if previous == Some(turn.status) {
            return;
        }
        let event = match turn.status {
            TurnStatus::Queued if previous.is_none() => {
                let Some(TurnContext::GithubReview(context)) = turn.context.as_ref() else {
                    return;
                };
                self.review_event(
                    turn,
                    StatusEvent::Requested {
                        user: context.trigger_user.clone(),
                        trigger: context.trigger,
                    },
                );
                StatusEvent::Queued
            }
            TurnStatus::Sending => {
                let Some(conversation) = self.conversations.get(&turn.conversation_id) else {
                    return;
                };
                StatusEvent::Reviewing {
                    provider: conversation.execution.provider.clone(),
                }
            }
            TurnStatus::Failed => StatusEvent::Failed {
                reason: turn
                    .error
                    .clone()
                    .unwrap_or_else(|| "The review could not be completed".to_owned()),
            },
            TurnStatus::Uncertain => StatusEvent::Unknown,
            TurnStatus::Cancelled => StatusEvent::Cancelled,
            _ => return,
        };
        self.review_event(turn, event);
    }

    /// Record an interrupted or failed publication, unless GitHub already confirmed it.
    pub(super) fn review_delivery_uncertain(&mut self, turn: &Turn) {
        if turn.status == TurnStatus::Completed && turn.delivery == DeliveryStatus::Uncertain {
            self.review_event(turn, StatusEvent::Unknown);
        }
    }

    /// Apply a verified PR update without admitting another review.
    pub(crate) fn review_head_changed(&mut self, key: &ReviewStatusKey, before: &str, after: &str) {
        if let Some(status) = self.review_status.get_mut(key)
            && status
                .heads
                .last()
                .is_some_and(|head| head.head_sha == before)
        {
            let revision = status.revision;
            status.observe_head(after, &Utc::now().to_rfc3339());
            if status.revision != revision {
                let _ = self.store.save_review_status(status);
            }
        }
    }

    /// A review of the same head was asked for again by the user of `turn`'s new request.
    pub(super) fn review_requested_again(&mut self, turn: &Turn, incoming: Option<&TurnContext>) {
        let Some(TurnContext::GithubReview(context)) = incoming else {
            return;
        };
        self.review_event(
            turn,
            StatusEvent::RequestedAgain {
                user: context.trigger_user.clone(),
                trigger: context.trigger,
                comment_id: trigger_comment(context),
            },
        );
    }

    /// Append an event the channel observed, such as a published review, to `turn`'s board.
    pub(crate) fn review_status_event(
        &mut self,
        turn: BotTurnId,
        event: StatusEvent,
    ) -> Result<(), ErrorCode> {
        let turn = self.turn(turn)?;
        self.review_event(&turn, event);
        Ok(())
    }

    /// Append `event` to the board of turn `id`, if it is a GitHub review.
    pub(super) fn review_event_by_id(&mut self, id: BotTurnId, event: StatusEvent) {
        if let Ok(turn) = self.turn(id) {
            self.review_event(&turn, event);
        }
    }

    /// The boards of `channel` that GitHub has yet to see, with the channel's configuration.
    pub(crate) fn review_status_pending(&self, channel: ChannelId) -> Vec<ReviewStatus> {
        self.review_status
            .values()
            .filter(|status| status.key.channel_id == channel && status.dirty())
            .cloned()
            .collect()
    }

    /// Record what the channel worker wrote.
    pub(crate) fn review_status_written(&mut self, written: &StatusWritten) {
        let Some(status) = self.review_status.get_mut(&written.key) else {
            return;
        };
        let mut updated = status.clone();
        updated.published_revision = updated.published_revision.max(written.revision);
        updated.pages.clone_from(&written.pages);
        updated
            .pending_reactions
            .retain(|comment| !written.reacted.contains(comment));
        if self.store.save_review_status(&updated).is_ok() {
            *status = updated;
        }
    }

    /// Drop the boards of a deleted channel.
    pub(super) fn review_status_released(&mut self, channel: ChannelId) {
        self.review_status
            .retain(|key, _| key.channel_id != channel);
        let _ = self.store.delete_review_statuses(channel);
    }

    fn review_event(&mut self, turn: &Turn, event: StatusEvent) {
        let Some((key, head)) = self.review_key(turn) else {
            return;
        };
        let comment = match &event {
            StatusEvent::Requested { .. } => turn.context.as_ref().and_then(|context| {
                let TurnContext::GithubReview(context) = context;
                trigger_comment(context)
            }),
            StatusEvent::RequestedAgain { comment_id, .. } => *comment_id,
            _ => None,
        };
        let status = self
            .review_status
            .entry(key.clone())
            .or_insert_with(|| ReviewStatus::new(key));
        if let Some(section) = status.heads.iter().find(|section| section.head_sha == head) {
            if let StatusEvent::RequestedAgain { comment_id: Some(id), .. } = event
                && section.lines.iter().any(|line| matches!(
                    line.event, StatusEvent::RequestedAgain { comment_id: Some(previous), .. } if previous == id
                ))
            {
                return;
            }
            let previous = section.lines.iter().rev().find(|line| {
                !matches!(
                    line.event,
                    StatusEvent::Outdated { .. } | StatusEvent::RequestedAgain { .. }
                )
            });
            if previous.is_some_and(|line| {
                line.event == event
                    || matches!(event, StatusEvent::Unknown)
                        && matches!(line.event, StatusEvent::Published { .. })
            }) {
                return;
            }
        }
        status.append(&head, event, &Utc::now().to_rfc3339());
        if let Some(comment) = comment {
            status.react(comment);
        }
        let _ = self.store.save_review_status(status);
    }

    /// The board and head of a review turn on a GitHub channel.
    fn review_key(&self, turn: &Turn) -> Option<(ReviewStatusKey, String)> {
        let Some(TurnContext::GithubReview(context)) = turn.context.as_ref() else {
            return None;
        };
        let channel = self.conversations.get(&turn.conversation_id)?.channel_id;
        let github = self.catalog.channels.iter().any(|configured| {
            configured.id == channel
                && !configured.deleted
                && matches!(configured.config, ChannelConfig::Github { .. })
        });
        github.then(|| {
            (
                ReviewStatusKey {
                    channel_id: channel,
                    repository: context.repository.clone(),
                    pull_number: context.pull_number,
                },
                context.head_sha.clone(),
            )
        })
    }
}

fn trigger_comment(context: &domain::bot::GithubReviewContext) -> Option<u64> {
    context.trigger_comment_id.as_deref()?.parse().ok()
}
