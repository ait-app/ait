use domain::bot::{ChannelConfig, Conversation, DeliveryStatus, Turn, TurnStatus};
use domain::{BotTurnId, ChannelId, ConversationId};
use model::ErrorCode;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{State, project_conversation};
use crate::channels::Incoming;
use crate::protocol::{BotRequest, ConversationRequest, History, MemoryUpdate, Send};

impl State {
    pub(super) fn open(&mut self, request: BotRequest) -> Result<Value, ErrorCode> {
        let bot = self.bot(request.bot_id)?;
        self.workspace(bot)?;
        let incoming = Incoming {
            channel_id: bot.builtin_channel_id,
            peer: "owner".into(),
            sender: "owner".into(),
            thread: None,
            message_id: String::new(),
            text: String::new(),
            reply_to: None,
        };
        let conversation = self.route(&incoming)?;
        Ok(json!({"conversation":project_conversation(&conversation)}))
    }
    pub(crate) fn route(&mut self, incoming: &Incoming) -> Result<Conversation, ErrorCode> {
        let channel = self
            .catalog
            .channels
            .iter()
            .find(|channel| channel.id == incoming.channel_id && !channel.deleted)
            .ok_or(ErrorCode::BotNotFound)?;
        self.workspace(self.bot(channel.bot_id)?)?;
        if !channel.enabled {
            return Err(ErrorCode::BotConflict);
        }
        if let Some(conversation) = self.conversations.values().find(|conversation| {
            conversation.channel_id == incoming.channel_id
                && conversation.peer == incoming.peer
                && conversation.sender == incoming.sender
                && conversation.thread == incoming.thread
        }) {
            return Ok(conversation.clone());
        }
        if self.conversations.len() >= 4096 {
            return Err(ErrorCode::ResourceExhausted);
        }
        let bot = self.bot(channel.bot_id)?;
        let conversation = Conversation {
            id: ConversationId::generate(),
            bot_id: channel.bot_id,
            channel_id: channel.id,
            peer: incoming.peer.clone(),
            sender: incoming.sender.clone(),
            thread: incoming.thread.clone(),
            session_id: uuid::Uuid::new_v4().to_string(),
            generation: 0,
            execution: bot.execution.clone(),
            instructions: bot.instructions.clone(),
            next_sequence: 1,
        };
        self.save_conversation(conversation.clone())?;
        Ok(conversation)
    }
    pub(super) fn send(&mut self, request: Send) -> Result<Value, ErrorCode> {
        let conversation = self
            .conversation(request.bot_id, request.conversation_id)?
            .clone();
        if !matches!(
            self.channel(request.bot_id, conversation.channel_id)?
                .config,
            ChannelConfig::Builtin
        ) {
            return Err(ErrorCode::BotConflict);
        }
        self.accept(Incoming {
            channel_id: conversation.channel_id,
            peer: conversation.peer,
            sender: conversation.sender,
            thread: conversation.thread,
            message_id: request.message_id,
            text: request.text,
            reply_to: None,
        })
    }
    pub(crate) fn accept(&mut self, incoming: Incoming) -> Result<Value, ErrorCode> {
        self.ensure_writable()?;
        if !model::valid_id(&incoming.message_id)
            || incoming.text.trim().is_empty()
            || incoming.text.len() > 16 * 1024
            || incoming.text.contains('\0')
            || !model::valid_id(&incoming.peer)
            || !model::valid_id(&incoming.sender)
            || incoming
                .thread
                .as_ref()
                .is_some_and(|thread| !model::valid_id(thread))
            || incoming
                .reply_to
                .as_ref()
                .is_some_and(|id| !model::valid_id(id))
        {
            return Err(ErrorCode::InvalidMessage);
        }
        if let Some(id) = self
            .inputs
            .get(&(incoming.channel_id, incoming.message_id.clone()))
        {
            let turn = self.turn(*id)?;
            let conversation = self
                .conversations
                .get(&turn.conversation_id)
                .ok_or(ErrorCode::BotIo)?;
            if conversation.peer != incoming.peer
                || conversation.sender != incoming.sender
                || conversation.thread != incoming.thread
                || turn.text != incoming.text
                || turn.reply_to != incoming.reply_to
            {
                return Err(ErrorCode::IdempotencyConflict);
            }
            return Ok(json!({"turn":turn,"accepted":true,"duplicate":true}));
        }
        let conversation = self.route(&incoming)?;
        if self.pending.len() + self.running.len() >= 512
            || self
                .pending
                .range((conversation.id, 0)..=(conversation.id, u64::MAX))
                .take(32)
                .count()
                >= 32
        {
            return Err(ErrorCode::ResourceExhausted);
        }
        let turn = Turn {
            id: BotTurnId::generate(),
            conversation_id: conversation.id,
            message_id: incoming.message_id,
            sequence: conversation.next_sequence,
            generation: conversation.generation,
            text: incoming.text,
            reply_to: incoming.reply_to,
            reply: None,
            pending_permissions: Vec::new(),
            status: TurnStatus::Queued,
            delivery: DeliveryStatus::Pending,
            error: None,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        let mut next = conversation;
        next.next_sequence = next
            .next_sequence
            .checked_add(1)
            .ok_or(ErrorCode::ResourceExhausted)?;
        self.save_conversation(next)?;
        self.commit_turn(&turn)?;
        self.inputs
            .insert((incoming.channel_id, turn.message_id.clone()), turn.id);
        Ok(json!({"turn":turn,"accepted":true,"duplicate":false}))
    }
    pub(super) fn reset(&mut self, request: ConversationRequest) -> Result<Value, ErrorCode> {
        self.workspace(self.bot(request.bot_id)?)?;
        let mut conversation = self
            .conversation(request.bot_id, request.conversation_id)?
            .clone();
        if self.running.contains_key(&conversation.id) {
            return Err(ErrorCode::BotConflict);
        }
        if self.receipts.values().any(|receipt| {
            receipt.conversation == conversation.id && receipt.status == TurnStatus::Queued
        }) {
            return Err(ErrorCode::BotConflict);
        }
        let owner = self.owner(&conversation)?;
        self.session_actions.push((
            owner,
            provider::service::bot_execution::Action::Close {
                agent_id: conversation.session_id.clone(),
            },
        ));
        conversation.session_id = uuid::Uuid::new_v4().to_string();
        conversation.generation = conversation
            .generation
            .checked_add(1)
            .ok_or(ErrorCode::ResourceExhausted)?;
        let bot = self.bot(request.bot_id)?;
        conversation.execution = bot.execution.clone();
        conversation.instructions.clone_from(&bot.instructions);
        self.save_conversation(conversation.clone())?;
        Ok(json!({"conversation":project_conversation(&conversation)}))
    }
    pub(super) fn history(&self, request: History) -> Result<Value, ErrorCode> {
        self.conversation(request.bot_id, request.conversation_id)?;
        let limit = request.limit.unwrap_or(20);
        if limit == 0 || limit > 50 {
            return Err(ErrorCode::InvalidMessage);
        }
        let selected = self
            .order
            .range(
                (
                    request.conversation_id,
                    request.after_sequence.saturating_add(1),
                )..=(request.conversation_id, u64::MAX),
            )
            .take(limit);
        let mut turns = Vec::with_capacity(limit);
        let mut bytes = 0;
        for (_, id) in selected {
            let turn = self.turn(*id)?;
            let size = serde_json::to_vec(&turn)
                .map_err(|_| ErrorCode::BotIo)?
                .len();
            if bytes + size > 2 * 1024 * 1024 {
                break;
            }
            bytes += size;
            turns.push(turn);
        }
        Ok(json!({"turns":turns,"nextSequence":turns.last().map(|turn| turn.sequence)}))
    }
    pub(super) fn memory(&self, request: BotRequest) -> Result<Value, ErrorCode> {
        let bot = self.bot(request.bot_id)?;
        let cwd = self.active_workspace(&bot.workspace_id)?;
        let text = self
            .store
            .memory(&cwd, &bot.workspace_id, bot.id)
            .map_err(|_| ErrorCode::BotIo)?;
        Ok(json!({"text":text,"revision":revision(&text)}))
    }
    pub(super) fn update_memory(&mut self, request: &MemoryUpdate) -> Result<Value, ErrorCode> {
        let bot = self.bot(request.bot_id)?;
        let cwd = self.workspace(bot)?;
        let text = self
            .store
            .memory(&cwd, &bot.workspace_id, bot.id)
            .map_err(|_| ErrorCode::BotIo)?;
        if revision(&text) != request.expected {
            return Err(ErrorCode::BotConflict);
        }
        if request.text.len() > 32 * 1024 || request.text.contains('\0') {
            return Err(ErrorCode::InvalidMessage);
        }
        if self
            .store
            .save_memory(&cwd, &bot.workspace_id, bot.id, &request.text)
            .is_err()
        {
            self.frozen = true;
            return Err(ErrorCode::BotIo);
        }
        Ok(json!({"revision":revision(&request.text)}))
    }
    pub(crate) fn advance_cursor(&mut self, id: ChannelId, cursor: i64) -> Result<(), ErrorCode> {
        let mut catalog = self.catalog.clone();
        let channel = catalog
            .channels
            .iter_mut()
            .find(|channel| channel.id == id)
            .ok_or(ErrorCode::BotNotFound)?;
        if channel.cursor.is_none_or(|current| current < cursor) {
            channel.cursor = Some(cursor);
            self.save_catalog(catalog)?;
        }
        Ok(())
    }
}

fn revision(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}
