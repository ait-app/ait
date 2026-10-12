mod catalog;
mod conversations;
mod execution;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use domain::bot::{
    Bot, Catalog, Channel, Conversation, DeliveryStatus, SessionOwner, Turn, TurnStatus,
};
use domain::{BotId, BotTurnId, ChannelId, ConversationId};
use model::ErrorCode;
use model::bot::Store;
use model::workspace::registry::{ProjectRegistry, WorkspaceRegistry};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::protocol::{self, decode};

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
}

impl State {
    pub(crate) fn load(
        store: Arc<dyn Store>,
        workspaces: Arc<dyn WorkspaceRegistry>,
        projects: Arc<dyn ProjectRegistry>,
    ) -> Result<Self, ErrorCode> {
        let catalog = store.catalog().map_err(|_| ErrorCode::BotIo)?;
        catalog::validate(&catalog)?;
        let mut state = Self {
            store,
            workspaces,
            projects,
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
        };
        for conversation in state.store.conversations().map_err(|_| ErrorCode::BotIo)? {
            state.channel(conversation.bot_id, conversation.channel_id)?;
            if state
                .conversations
                .insert(conversation.id, conversation.clone())
                .is_some()
            {
                return Err(ErrorCode::BotIo);
            }
            let store = state.store.clone();
            store.scan_turns(conversation.id, &mut |mut turn| {
                let original_status = turn.status;
                let original_delivery = turn.delivery;
                if turn.conversation_id != conversation.id
                    || turn.sequence >= conversation.next_sequence
                {
                    return Err(model::bot::StorageError);
                }
                if matches!(
                    turn.status,
                    TurnStatus::Sending | TurnStatus::Running | TurnStatus::NeedsApproval
                ) {
                    turn.status = TurnStatus::Uncertain;
                    turn.error = Some("Daemon restarted during native execution; reset the conversation to continue".into());
                }
                if turn.delivery == DeliveryStatus::Sending {
                    turn.delivery = DeliveryStatus::Uncertain;
                }
                if state.receipts.contains_key(&turn.id)
                    || state.order.contains_key(&(conversation.id, turn.sequence))
                    || state
                        .inputs
                        .contains_key(&(conversation.channel_id, turn.message_id.clone()))
                {
                    return Err(model::bot::StorageError);
                }
                if matches!(original_status, TurnStatus::Sending | TurnStatus::Running | TurnStatus::NeedsApproval)
                    || original_delivery == DeliveryStatus::Sending {
                    state.store.save_turn(&turn)?;
                }
                state.index_turn(&turn).map_err(|_| model::bot::StorageError)?;
                state
                    .inputs
                    .insert((conversation.channel_id, turn.message_id.clone()), turn.id);
                Ok(())
            }).map_err(|_| ErrorCode::BotIo)?;
        }
        Ok(state)
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
            "bot.create.request" => self.create(decode(params)?),
            "bot.list.request" => {
                let request: protocol::List = decode(params)?;
                Ok(
                    json!({"bots":self.catalog.bots.iter().filter(|bot| bot.workspace_id == request.workspace_id)
                    .map(|bot| json!({"id":bot.id,"workspaceId":bot.workspace_id,"name":bot.name,"execution":bot.execution,
                        "enabled":bot.enabled,"archivedAt":bot.archived_at,"builtinChannelId":bot.builtin_channel_id,"createdAt":bot.created_at})).collect::<Vec<_>>()}),
                )
            }
            "bot.get.request" => {
                let request: protocol::BotRequest = decode(params)?;
                Ok(json!({"bot":self.bot(request.bot_id)?}))
            }
            "bot.update.request" => self.update(decode(params)?),
            "bot.archive.request" => self.archive(decode(params)?),
            "bot.channel.create.request" => self.create_channel(decode(params)?),
            "bot.channel.update.request" => self.update_channel(decode(params)?),
            "bot.channel.delete.request" => self.delete_channel(decode(params)?),
            "bot.channel.list.request" => {
                let request: protocol::BotRequest = decode(params)?;
                self.bot(request.bot_id)?;
                Ok(
                    json!({"channels":self.catalog.channels.iter().filter(|channel| channel.bot_id == request.bot_id && !channel.deleted).collect::<Vec<_>>()}),
                )
            }
            "bot.channel.status.request" => {
                let request: protocol::ChannelRequest = decode(params)?;
                let channel = self.channel(request.bot_id, request.channel_id)?;
                Ok(
                    json!({"channelId":channel.id,"enabled":channel.enabled,"connection":self.channel_status.get(&channel.id)}),
                )
            }
            "bot.conversation.open.request" => self.open(decode(params)?),
            "bot.conversation.list.request" => {
                let request: protocol::BotRequest = decode(params)?;
                self.bot(request.bot_id)?;
                Ok(
                    json!({"conversations":self.conversations.values().filter(|conversation| conversation.bot_id == request.bot_id).map(project_conversation).collect::<Vec<_>>()}),
                )
            }
            "bot.conversation.reset.request" => self.reset(decode(params)?),
            "bot.message.send.request" => self.send(decode(params)?),
            "bot.history.get.request" => self.history(decode(params)?),
            "bot.turn.get.request" => {
                let request: protocol::TurnRequest = decode(params)?;
                Ok(json!({"turn":self.owned_turn(request.bot_id, request.turn_id)?}))
            }
            "bot.memory.get.request" => self.memory(decode(params)?),
            "bot.memory.update.request" => self.update_memory(&decode(params)?),
            _ => Err(ErrorCode::MethodNotFound),
        }
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
        if !bot.enabled || bot.archived_at.is_some() {
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

fn project_conversation(conversation: &Conversation) -> Value {
    json!({"id":conversation.id,"botId":conversation.bot_id,"channelId":conversation.channel_id,
        "peer":conversation.peer,"sender":conversation.sender,"thread":conversation.thread,"generation":conversation.generation,"nextSequence":conversation.next_sequence})
}

#[cfg(test)]
mod tests;
