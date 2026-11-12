mod catalog;
mod conversations;
mod execution;
mod notifications;
mod tickets;
mod webhook;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use domain::bot::ChannelConfig;
use domain::bot::notification::NotificationLog;
use domain::bot::ticket::Ticket;
use domain::bot::{
    Bot, BotState, Catalog, Channel, Conversation, DeliveryStatus, SessionOwner, Turn, TurnStatus,
};
use domain::{BotId, BotTurnId, ChannelId, ConversationId, TicketId};
use model::ErrorCode;
use model::bot::Store;
use model::workspace::registry::{ProjectRegistry, WorkspaceRegistry};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::mcp::Scopes;
use crate::protocol::{self, decode};

/// A verified external account whose secret the daemon stores on the channel's behalf.
pub(crate) struct StoredChannel {
    pub(crate) workspace_id: String,
    pub(crate) channel_id: ChannelId,
    pub(crate) config: ChannelConfig,
}

#[derive(Debug)]
pub(crate) struct Receipt {
    conversation: ConversationId,
    pub(crate) status: TurnStatus,
    generation: u64,
}

pub(crate) struct State {
    pub(crate) store: Arc<dyn Store>,
    workspaces: Arc<dyn WorkspaceRegistry>,
    projects: Arc<dyn ProjectRegistry>,
    scopes: Arc<Scopes>,
    catalog: Catalog,
    conversations: BTreeMap<ConversationId, Conversation>,
    receipts: BTreeMap<BotTurnId, Receipt>,
    order: BTreeMap<(ConversationId, u64), BotTurnId>,
    inputs: BTreeMap<(ChannelId, String), BotTurnId>,
    pending: BTreeMap<(ConversationId, u64), BotTurnId>,
    uncertain: BTreeSet<(ConversationId, u64)>,
    outbox: BTreeMap<ChannelId, BTreeMap<(ConversationId, u64), BotTurnId>>,
    pub(crate) running: BTreeMap<ConversationId, CancellationToken>,
    pub(crate) channel_status: BTreeMap<ChannelId, Value>,
    pub(crate) frozen: bool,
    pub(crate) session_actions: Vec<(SessionOwner, provider::service::bot_execution::Action)>,
    /// Deleted channels whose stored secrets the runtime has yet to remove.
    pub(crate) released_credentials: Vec<ChannelId>,
    /// Deleted webhook channels whose center endpoints the runtime has yet to remove.
    pub(crate) released_webhooks: Vec<ChannelId>,
    /// Notification log and inbox of each Bot that has received a notification.
    notifications: BTreeMap<BotId, NotificationLog>,
    /// Start times of recent wake turns per conversation, for the wake budget.
    wakes: BTreeMap<ConversationId, Vec<DateTime<Utc>>>,
    /// Newest notification sequence each conversation had in front of it in a completed turn.
    seen: BTreeMap<ConversationId, u64>,
    /// When a completed turn of each conversation last read each Agent settled, by the Agent's
    /// own clock.
    observations: BTreeMap<(ConversationId, String), DateTime<Utc>>,
    /// Reads of settled Agents by each conversation's running turn, applied to
    /// `observations` only if that turn completes.
    observing: BTreeMap<ConversationId, notifications::Observing>,
    /// Notification ids each Bot's recent wake turns carried, with the turn's start, so a
    /// replay stays a duplicate after the log evicted the entry.
    woken: BTreeMap<(BotId, String), DateTime<Utc>>,
    /// Every live Bot's tickets, each with its newest report only; the files hold the rest.
    tickets: BTreeMap<TicketId, Ticket>,
    /// The service is stopping: running turns are being cancelled by shutdown, not by a user.
    pub(crate) stopping: bool,
}

impl State {
    pub(crate) fn load(
        store: Arc<dyn Store>,
        workspaces: Arc<dyn WorkspaceRegistry>,
        projects: Arc<dyn ProjectRegistry>,
        scopes: Arc<Scopes>,
    ) -> Result<Self, ErrorCode> {
        let mut catalog = store.catalog().map_err(|_| ErrorCode::BotIo)?;
        catalog::validate(&catalog)?;
        // Bots run in their provider's read-only mode; a mode stored by an earlier build is
        // dropped here and persisted with the next catalog save, as are permission entries
        // that no longer apply.
        for bot in &mut catalog.bots {
            bot.execution.mode_id = None;
            bot.permissions = std::mem::take(&mut bot.permissions)
                .normalized()
                .map_err(|_| ErrorCode::BotIo)?;
        }
        scopes.publish(&catalog);
        // Retried on every start so a crash after a deletion cannot strand its secret.
        let released_credentials = catalog
            .channels
            .iter()
            .filter(|channel| channel.deleted)
            .map(|channel| channel.id)
            .collect();
        let now = Utc::now();
        let notifications = notifications::load(store.as_ref(), &catalog)?;
        let loaded = tickets::load(store.as_ref(), &catalog, now)?;
        let mut state = Self {
            store,
            workspaces,
            projects,
            scopes,
            catalog,
            conversations: BTreeMap::new(),
            receipts: BTreeMap::new(),
            order: BTreeMap::new(),
            inputs: BTreeMap::new(),
            pending: BTreeMap::new(),
            uncertain: BTreeSet::new(),
            outbox: BTreeMap::new(),
            running: BTreeMap::new(),
            channel_status: BTreeMap::new(),
            frozen: false,
            session_actions: Vec::new(),
            released_credentials,
            released_webhooks: Vec::new(),
            notifications,
            wakes: BTreeMap::new(),
            seen: BTreeMap::new(),
            observations: BTreeMap::new(),
            observing: BTreeMap::new(),
            woken: BTreeMap::new(),
            tickets: loaded.tickets,
            stopping: false,
        };
        state.load_turns(now)?;
        state.offer_reports(loaded.reports, now)?;
        Ok(state)
    }

    /// Index every stored turn, mark those a stopped daemon left executing `uncertain`, and
    /// put the notifications of interrupted wake turns back in their inbox, as of `now`.
    fn load_turns(&mut self, now: DateTime<Utc>) -> Result<(), ErrorCode> {
        let mut stored_wakes = Vec::new();
        let mut rewritten = Vec::new();
        for conversation in self.store.conversations().map_err(|_| ErrorCode::BotIo)? {
            self.channel(conversation.bot_id, conversation.channel_id)?;
            if self
                .conversations
                .insert(conversation.id, conversation.clone())
                .is_some()
            {
                return Err(ErrorCode::BotIo);
            }
            let store = self.store.clone();
            store.scan_turns(conversation.id, &mut |mut turn| {
                let interrupted = matches!(
                    turn.status,
                    TurnStatus::Sending | TurnStatus::Running | TurnStatus::NeedsApproval
                );
                let original_delivery = turn.delivery;
                if turn.conversation_id != conversation.id
                    || turn.sequence >= conversation.next_sequence
                {
                    return Err(model::bot::StorageError);
                }
                if interrupted {
                    turn.status = TurnStatus::Uncertain;
                    turn.error = Some("Daemon restarted during native execution; reset the conversation to continue".into());
                }
                if turn.delivery == DeliveryStatus::Sending {
                    turn.delivery = DeliveryStatus::Uncertain;
                }
                if self.receipts.contains_key(&turn.id)
                    || self.order.contains_key(&(conversation.id, turn.sequence))
                    || self
                        .inputs
                        .contains_key(&(conversation.channel_id, turn.message_id.clone()))
                {
                    return Err(model::bot::StorageError);
                }
                self.index_turn(&turn).map_err(|_| model::bot::StorageError)?;
                self
                    .inputs
                    .insert((conversation.channel_id, turn.message_id.clone()), turn.id);
                self.load_wake(&conversation, &turn, interrupted, now, &mut stored_wakes);
                if turn.status == TurnStatus::Completed {
                    self.saw_notifications(&turn);
                }
                if interrupted || original_delivery == DeliveryStatus::Sending {
                    rewritten.push(turn);
                }
                Ok(())
            }).map_err(|_| ErrorCode::BotIo)?;
        }
        // The inbox goes first: a crash before the rewrites leaves an interrupted wake turn
        // executing on disk with its notifications already pending, and the next start finds
        // nothing left to requeue, so the retry is neither lost nor repeated.
        self.reconcile_wakes(stored_wakes)?;
        for turn in &rewritten {
            self.store.save_turn(turn).map_err(|_| ErrorCode::BotIo)?;
        }
        Ok(())
    }

    pub(crate) fn execute(&mut self, method: &str, params: Value) -> Result<Value, ErrorCode> {
        if self.frozen {
            return Err(ErrorCode::BotIo);
        }
        match method {
            "bot.channel.types.request" => {
                let _: protocol::Empty = decode(params)?;
                Ok(
                    json!({"supported":["builtin","telegram","feishu"],"email":{"supported":false,"planned":true}}),
                )
            }
            "bot.get.request" => {
                let request: protocol::WorkspaceRequest = decode(params)?;
                Ok(json!({"bot":self.current(&request.workspace_id).ok().map(view)}))
            }
            "bot.setup.request" => self.setup(decode(params)?),
            "bot.update.request" => self.update(decode(params)?),
            "bot.permissions.update.request" => self.update_permissions(decode(params)?),
            "bot.pause.request" => self.pause(&decode(params)?),
            "bot.resume.request" => self.resume(&decode(params)?),
            "bot.remove.request" => self.remove(&decode(params)?),
            "bot.channel.create.request" => {
                let request: protocol::ChannelCreate = decode(params)?;
                // Only the center issues webhook endpoints; see `channels::webhook`.
                if matches!(request.config, ChannelConfig::Webhook { .. }) {
                    return Err(ErrorCode::InvalidMessage);
                }
                self.create_channel(request)
            }
            "bot.channel.update.request" => self.update_channel(decode(params)?),
            "bot.channel.delete.request" => self.delete_channel(&decode(params)?),
            "bot.channel.list.request" => {
                let request: protocol::WorkspaceRequest = decode(params)?;
                let bot = self.current(&request.workspace_id)?.id;
                Ok(
                    json!({"channels":self.catalog.channels.iter().filter(|channel| channel.bot_id == bot && !channel.deleted).map(project_channel).collect::<Vec<_>>()}),
                )
            }
            "bot.channel.status.request" => {
                let request: protocol::ChannelRequest = decode(params)?;
                let bot = self.current(&request.workspace_id)?.id;
                let channel = self.channel(bot, request.channel_id)?;
                Ok(
                    json!({"channelId":channel.id,"enabled":channel.enabled,"connection":self.channel_status.get(&channel.id)}),
                )
            }
            "bot.conversation.open.request" => self.open(&decode(params)?),
            "bot.conversation.list.request" => {
                let request: protocol::WorkspaceRequest = decode(params)?;
                let bot = self.current(&request.workspace_id)?.id;
                Ok(
                    json!({"conversations":self.conversations.values().filter(|conversation| conversation.bot_id == bot).map(project_conversation).collect::<Vec<_>>()}),
                )
            }
            "bot.conversation.reset.request" => self.reset(&decode(params)?),
            "bot.message.send.request" => self.send(decode(params)?),
            "bot.history.get.request" => self.history(&decode(params)?),
            "bot.turn.get.request" => {
                let request: protocol::TurnRequest = decode(params)?;
                let bot = self.current(&request.workspace_id)?.id;
                Ok(json!({"turn":self.owned_turn(bot, request.turn_id)?}))
            }
            "bot.memory.get.request" => self.memory(&decode(params)?),
            "bot.notifications.list.request" => self.list_notifications(&decode(params)?),
            "bot.ticket.list.request" => self.list_tickets(&decode(params)?),
            "bot.ticket.get.request" => self.get_ticket(&decode(params)?),
            "bot.ticket.update.request" => self.user_update_ticket(decode(params)?, Utc::now()),
            "bot.memory.update.request" => self.update_memory(&decode(params)?),
            "bot.webhook.message.send.request" => self.webhook_send(decode(params)?),
            "bot.webhook.delivery.ack.request" => self.webhook_ack(&decode(params)?),
            "bot.webhook.conversation.close.request" => self.webhook_close(&decode(params)?),
            _ => Err(ErrorCode::MethodNotFound),
        }
    }

    /// The Workspace's single non-retired Bot.
    fn current(&self, workspace: &str) -> Result<&Bot, ErrorCode> {
        self.catalog
            .bots
            .iter()
            .find(|bot| {
                bot.workspace_id == workspace && !matches!(bot.state, BotState::Retired { .. })
            })
            .ok_or(ErrorCode::BotNotFound)
    }
    fn bot(&self, id: BotId) -> Result<&Bot, ErrorCode> {
        self.catalog
            .bots
            .iter()
            .find(|bot| bot.id == id)
            .ok_or(ErrorCode::BotNotFound)
    }
    fn channel(&self, bot: BotId, id: ChannelId) -> Result<&Channel, ErrorCode> {
        self.catalog
            .channels
            .iter()
            .find(|channel| channel.id == id && channel.bot_id == bot)
            .ok_or(ErrorCode::BotNotFound)
    }
    fn workspace(&self, bot: &Bot) -> Result<String, ErrorCode> {
        if !matches!(bot.state, BotState::Active) {
            return Err(ErrorCode::BotConflict);
        }
        self.active_workspace(&bot.workspace_id)
    }
    fn active_workspace(&self, id: &str) -> Result<String, ErrorCode> {
        let workspace = self
            .workspaces
            .get(id)
            .map_err(|_| ErrorCode::RegistryIo)?
            .ok_or(ErrorCode::WorkspaceNotFound)?;
        let project = self
            .projects
            .get(&workspace.project_id)
            .map_err(|_| ErrorCode::RegistryIo)?
            .ok_or(ErrorCode::WorkspaceNotFound)?;
        if workspace
            .archived_at
            .as_ref()
            .is_some_and(|value| !value.is_empty())
            || project
                .archived_at
                .as_ref()
                .is_some_and(|value| !value.is_empty())
        {
            return Err(ErrorCode::BotConflict);
        }
        Ok(workspace.cwd)
    }
    fn save_catalog(&mut self, catalog: Catalog) -> Result<(), ErrorCode> {
        self.ensure_writable()?;
        catalog::validate(&catalog)?;
        if self.store.save_catalog(&catalog).is_err() {
            self.frozen = true;
            return Err(ErrorCode::BotIo);
        }
        self.scopes.publish(&catalog);
        self.catalog = catalog;
        Ok(())
    }
    fn save_conversation(&mut self, conversation: Conversation) -> Result<(), ErrorCode> {
        self.ensure_writable()?;
        if self.store.save_conversation(&conversation).is_err() {
            self.frozen = true;
            return Err(ErrorCode::BotIo);
        }
        self.conversations.insert(conversation.id, conversation);
        Ok(())
    }
    pub(crate) fn commit_turn(&mut self, turn: &Turn) -> Result<(), ErrorCode> {
        self.ensure_writable()?;
        if self.store.save_turn(turn).is_err() {
            self.frozen = true;
            return Err(ErrorCode::BotIo);
        }
        self.index_turn(turn)
    }
    fn ensure_writable(&self) -> Result<(), ErrorCode> {
        if self.frozen {
            Err(ErrorCode::BotIo)
        } else {
            Ok(())
        }
    }
    fn index_turn(&mut self, turn: &Turn) -> Result<(), ErrorCode> {
        self.receipts.insert(
            turn.id,
            Receipt {
                conversation: turn.conversation_id,
                status: turn.status,
                generation: turn.generation,
            },
        );
        self.order
            .insert((turn.conversation_id, turn.sequence), turn.id);
        if turn.status == TurnStatus::Queued {
            self.pending
                .insert((turn.conversation_id, turn.sequence), turn.id);
        } else {
            self.pending.remove(&(turn.conversation_id, turn.sequence));
        }
        if turn.status == TurnStatus::Uncertain {
            self.uncertain
                .insert((turn.conversation_id, turn.generation));
        }
        let channel = self
            .conversations
            .get(&turn.conversation_id)
            .ok_or(ErrorCode::BotIo)?
            .channel_id;
        let outbox = self.outbox.entry(channel).or_default();
        if turn.status == TurnStatus::Completed && turn.delivery == DeliveryStatus::Pending {
            outbox.insert((turn.conversation_id, turn.sequence), turn.id);
        } else {
            outbox.remove(&(turn.conversation_id, turn.sequence));
        }
        Ok(())
    }
    pub(crate) fn turn(&self, id: BotTurnId) -> Result<Turn, ErrorCode> {
        let receipt = self.receipts.get(&id).ok_or(ErrorCode::BotNotFound)?;
        self.store
            .turn(receipt.conversation, id)
            .map_err(|_| ErrorCode::BotIo)
    }
    pub(crate) fn owned_turn(&self, bot: BotId, id: BotTurnId) -> Result<Turn, ErrorCode> {
        let turn = self.turn(id)?;
        self.conversation(bot, turn.conversation_id)?;
        Ok(turn)
    }
    pub(crate) fn conversation(
        &self,
        bot: BotId,
        id: ConversationId,
    ) -> Result<&Conversation, ErrorCode> {
        self.conversations
            .get(&id)
            .filter(|conversation| conversation.bot_id == bot)
            .ok_or(ErrorCode::BotNotFound)
    }
    pub(crate) fn owner(&self, conversation: &Conversation) -> Result<SessionOwner, ErrorCode> {
        Ok(SessionOwner {
            bot_id: conversation.bot_id,
            conversation_id: conversation.id,
            workspace_id: self.bot(conversation.bot_id)?.workspace_id.clone(),
        })
    }
    pub(crate) fn channels(&self) -> Vec<Channel> {
        self.catalog
            .channels
            .iter()
            .filter(|channel| {
                channel.enabled
                    && !channel.deleted
                    && self
                        .bot(channel.bot_id)
                        .is_ok_and(|bot| self.workspace(bot).is_ok())
            })
            .cloned()
            .collect()
    }
}

/// A new queued receipt for the next input of `conversation`.
fn queued_turn(
    conversation: &Conversation,
    message_id: String,
    text: String,
    now: DateTime<Utc>,
) -> Turn {
    Turn {
        id: BotTurnId::generate(),
        conversation_id: conversation.id,
        message_id,
        sequence: conversation.next_sequence,
        generation: conversation.generation,
        text,
        reply_to: None,
        reply: None,
        messages: Vec::new(),
        pending_permissions: Vec::new(),
        status: TurnStatus::Queued,
        delivery: DeliveryStatus::Pending,
        error: None,
        created_at: now.to_rfc3339(),
        wake: None,
        silent: false,
        notification_cursor: None,
    }
}

/// Lowercase hex SHA-256 digest of `text`, used for memory revisions and instruction digests.
pub(crate) fn sha256_hex(text: &str) -> String {
    format!(
        "{:x}",
        <sha2::Sha256 as sha2::Digest>::digest(text.as_bytes())
    )
}

/// Client projection of a Bot; the internal storage identity is never exposed.
fn view(bot: &Bot) -> Value {
    json!({"workspaceId":bot.workspace_id,"execution":bot.execution,
        "builtinChannelId":bot.builtin_channel_id,"state":bot.state,"createdAt":bot.created_at,
        "permissions":bot.permissions,"wakeScope":bot.wake_scope})
}

/// Client projection of a channel; the owning Bot's storage identity is never exposed.
fn project_channel(channel: &Channel) -> Value {
    json!({"id":channel.id,"config":channel.config,"enabled":channel.enabled,
        "cursor":channel.cursor,"deleted":channel.deleted})
}

/// Client projection of a conversation without the owning Bot or native session identity.
fn project_conversation(conversation: &Conversation) -> Value {
    json!({"id":conversation.id,"channelId":conversation.channel_id,
        "peer":conversation.peer,"sender":conversation.sender,"thread":conversation.thread,"generation":conversation.generation,"nextSequence":conversation.next_sequence,
        "sessionAgentId":conversation.session_id})
}

#[cfg(test)]
mod tests;
