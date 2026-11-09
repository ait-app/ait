//! The Ait center's channels: a Hub channel, and webhook channels whose endpoint the
//! center hosts. The center posts input and pulls replies over its own Hub session, so
//! these channels have no transport in the daemon.

use domain::bot::{BotState, Channel, ChannelConfig, DeliveryStatus, TurnStatus};
use domain::{BotId, BotTurnId, ChannelId, ConversationId};
use model::ErrorCode;
use serde_json::{Value, json};

use super::State;
use crate::channels::Incoming;
use crate::protocol::{ConversationRequest, HubAck, HubDelivery, HubSend};
use crate::service::HubBot;

impl State {
    /// One entry per enabled center channel whose Bot is not retired, in catalog order.
    pub(crate) fn hub_bots(&self) -> Vec<HubBot> {
        self.catalog
            .channels
            .iter()
            .filter(|channel| is_open_hub(channel))
            .filter_map(|channel| {
                let bot = self.bot(channel.bot_id).ok()?;
                let state = match bot.state {
                    BotState::Active => "active",
                    BotState::Paused { .. } => "paused",
                    BotState::Retired { .. } => return None,
                };
                let workspace = self.workspaces.get(&bot.workspace_id).ok().flatten()?;
                let title = workspace
                    .title
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or(workspace.display_name);
                Some(HubBot {
                    workspace_id: bot.workspace_id.clone(),
                    channel_id: channel.id,
                    title,
                    state,
                    kind: if matches!(channel.config, ChannelConfig::Webhook { .. }) {
                        "webhook"
                    } else {
                        "hub"
                    },
                })
            })
            .collect()
    }

    /// The Workspace's current Bot.
    fn hub_bot(&self, workspace: &str) -> Result<BotId, ErrorCode> {
        self.current(workspace)
            .map(|bot| bot.id)
            .map_err(|_| ErrorCode::Forbidden)
    }

    /// `channel`, if it is an enabled center channel of `bot`.
    fn hub_channel(&self, bot: BotId, channel: ChannelId) -> Result<&Channel, ErrorCode> {
        self.catalog
            .channels
            .iter()
            .find(|entry| entry.id == channel && entry.bot_id == bot && is_open_hub(entry))
            .ok_or(ErrorCode::Forbidden)
    }

    /// Refuse a turn outside the Workspace's enabled center channels.
    pub(crate) fn hub_turn(&self, workspace: &str, turn: BotTurnId) -> Result<(), ErrorCode> {
        let bot = self.hub_bot(workspace)?;
        let turn = self
            .owned_turn(bot, turn)
            .map_err(|_| ErrorCode::Forbidden)?;
        let conversation = self
            .conversations
            .get(&turn.conversation_id)
            .ok_or(ErrorCode::Forbidden)?;
        self.hub_channel(bot, conversation.channel_id).map(drop)
    }

    pub(super) fn hub_send(&mut self, request: HubSend) -> Result<Value, ErrorCode> {
        let bot = self.hub_bot(&request.workspace_id)?;
        let channel = self.hub_channel(bot, request.channel_id)?.id;
        self.accept_channel(Incoming {
            channel_id: channel,
            peer: request.peer,
            sender: request.sender,
            thread: Some(request.thread),
            message_id: request.message_id,
            text: request.text,
            reply_to: None,
        })
    }

    pub(super) fn hub_ack(&mut self, request: &HubAck) -> Result<Value, ErrorCode> {
        self.hub_turn(&request.workspace_id, request.turn_id)?;
        let turn = self.turn(request.turn_id)?;
        if turn.status != TurnStatus::Completed {
            return Err(ErrorCode::BotConflict);
        }
        let status = match request.status {
            HubDelivery::Delivered => DeliveryStatus::Delivered,
            HubDelivery::Uncertain => DeliveryStatus::Uncertain,
        };
        // A delivered reply stays delivered; repeating an acknowledgement changes nothing.
        if turn.delivery != status && turn.delivery != DeliveryStatus::Delivered {
            self.delivery(turn.id, status)?;
        }
        Ok(json!({}))
    }

    /// Forget a Hub conversation: its queued input is cancelled, its native session closed,
    /// and its records removed, so it stops counting toward the conversation limit.
    pub(super) fn hub_close(&mut self, request: &ConversationRequest) -> Result<Value, ErrorCode> {
        let bot = self.hub_bot(&request.workspace_id)?;
        let Some(conversation) = self
            .conversations
            .get(&request.conversation_id)
            .filter(|conversation| conversation.bot_id == bot)
            .cloned()
        else {
            return Ok(json!({}));
        };
        self.hub_channel(bot, conversation.channel_id)?;
        if self.running.contains_key(&conversation.id) {
            return Err(ErrorCode::BotConflict);
        }
        let owner = self.owner(&conversation)?;
        self.ensure_writable()?;
        if self.store.delete_conversation(conversation.id).is_err() {
            self.frozen = true;
            return Err(ErrorCode::BotIo);
        }
        self.forget(conversation.id);
        self.session_actions.push((
            owner,
            provider::service::bot_execution::Action::Close {
                agent_id: conversation.session_id,
            },
        ));
        Ok(json!({}))
    }

    /// Drop every index entry of a deleted conversation.
    fn forget(&mut self, conversation: ConversationId) {
        let range = (conversation, 0)..=(conversation, u64::MAX);
        let turns: Vec<BotTurnId> = self.order.range(range.clone()).map(|(_, id)| *id).collect();
        for id in &turns {
            self.receipts.remove(id);
        }
        self.order.retain(|key, _| key.0 != conversation);
        self.pending.retain(|key, _| key.0 != conversation);
        self.uncertain.retain(|key| key.0 != conversation);
        for outbox in self.outbox.values_mut() {
            outbox.retain(|key, _| key.0 != conversation);
        }
        self.inputs.retain(|_, id| !turns.contains(id));
        self.conversations.remove(&conversation);
    }
}

/// An enabled channel the center reaches: the Hub channel or a webhook channel.
fn is_open_hub(channel: &Channel) -> bool {
    channel.enabled
        && !channel.deleted
        && matches!(
            channel.config,
            ChannelConfig::Hub | ChannelConfig::Webhook { .. }
        )
}
