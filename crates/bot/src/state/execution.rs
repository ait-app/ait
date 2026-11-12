use std::collections::BTreeSet;
use std::path::Path;

use domain::bot::{
    Bot, BotState, ChannelConfig, DeliveryStatus, GithubReviewContext, PauseReason, Turn,
    TurnContext, TurnMessage, TurnMessageStatus, TurnStatus,
};
use domain::{BotId, BotTurnId, ConversationId};
use model::ErrorCode;
use provider::service::bot_execution::Action;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::State;
use crate::execution::Run;
use crate::protocol::TurnRequest;

pub(crate) struct ReviewPreparation {
    pub(crate) turn_id: BotTurnId,
    pub(crate) context: GithubReviewContext,
}

impl State {
    /// Cancel the Bot's running and queued turns and close or release its native sessions.
    ///
    /// `cancellation` becomes the error text of every queued turn cancelled here.
    pub(crate) fn stop_bot(
        &mut self,
        bot: BotId,
        retire: bool,
        cancellation: Option<&str>,
    ) -> Result<(), ErrorCode> {
        let conversations: Vec<_> = self
            .conversations
            .values()
            .filter(|conversation| conversation.bot_id == bot)
            .cloned()
            .collect();
        for conversation in conversations {
            if let Some(cancel) = self.running.get(&conversation.id) {
                cancel.cancel();
            }
            let action = if retire {
                Action::Close {
                    agent_id: conversation.session_id.clone(),
                }
            } else {
                Action::Release {
                    agent_id: conversation.session_id.clone(),
                }
            };
            self.session_actions
                .push((self.owner(&conversation)?, action));
            let queued: Vec<_> = self
                .pending
                .range((conversation.id, 0)..=(conversation.id, u64::MAX))
                .map(|(_, id)| *id)
                .collect();
            for id in queued {
                self.progress(
                    id,
                    TurnStatus::Cancelled,
                    None,
                    cancellation.map(str::to_owned),
                )?;
            }
        }
        Ok(())
    }
    /// Retire the archived Workspaces' Bots, drop them from every Bot's created worktrees and
    /// delete those Workspaces' Bot memory.
    pub(crate) fn retire(&mut self, workspaces: &[String]) -> Result<(), ErrorCode> {
        self.ensure_writable()?;
        let workspaces: BTreeSet<&str> = workspaces.iter().map(String::as_str).collect();
        let bots: BTreeSet<BotId> = self
            .catalog
            .bots
            .iter()
            .filter(|bot| {
                workspaces.contains(bot.workspace_id.as_str())
                    && !matches!(bot.state, BotState::Retired { .. })
            })
            .map(|bot| bot.id)
            .collect();
        let archived = |created: &String| workspaces.contains(created.as_str());
        let pruned = self
            .catalog
            .bots
            .iter()
            .any(|bot| bot.workspaces.iter().any(archived));
        if pruned || !bots.is_empty() {
            let mut catalog = self.catalog.clone();
            for bot in &mut catalog.bots {
                bot.workspaces.retain(|created| !archived(created));
            }
            if bots.is_empty() {
                self.save_catalog(catalog)?;
            } else {
                self.retire_bots(catalog, &bots)?;
            }
        }
        for workspace in workspaces {
            if self.store.delete_memory(workspace).is_err() {
                self.frozen = true;
                return Err(ErrorCode::BotIo);
            }
        }
        Ok(())
    }
    pub(crate) fn ready(&mut self) -> Result<Vec<Run>, ErrorCode> {
        if self.frozen {
            return Ok(Vec::new());
        }
        let mut ready = Vec::new();
        let mut failed = Vec::new();
        let mut missing = BTreeSet::new();
        let mut blocked = BTreeSet::new();
        for ((conversation, _), id) in &self.pending {
            let receipt = self.receipts.get(id).ok_or(ErrorCode::BotIo)?;
            if self
                .conversations
                .get(conversation)
                .is_none_or(|conversation| conversation.generation != receipt.generation)
            {
                continue;
            }
            if self.running.contains_key(conversation) {
                continue;
            }
            if self
                .uncertain
                .contains(&(*conversation, receipt.generation))
            {
                continue;
            }
            if receipt.status != TurnStatus::Queued || blocked.contains(conversation) {
                continue;
            }
            blocked.insert(*conversation);
            if self.running.len() + ready.len() >= 32 {
                break;
            }
            let conversation = self
                .conversations
                .get(conversation)
                .ok_or(ErrorCode::BotIo)?
                .clone();
            if receipt_context(&self.turn(*id)?).is_some()
                && conversation.execution_workspace_id.is_none()
            {
                continue;
            }
            if let Some(workspace_id) = conversation.execution_workspace_id.as_deref()
                && self.active_workspace(workspace_id).is_err()
            {
                failed.push(*id);
                continue;
            }
            let bot = self.bot(conversation.bot_id)?.clone();
            if missing.contains(&bot.id) {
                continue;
            }
            let execution_workspace = conversation
                .execution_workspace_id
                .as_deref()
                .unwrap_or(&bot.workspace_id);
            let Ok(cwd) = self.active_workspace(execution_workspace) else {
                continue;
            };
            if !Path::new(&cwd).is_dir() {
                if conversation.execution_workspace_id.is_some() {
                    failed.push(*id);
                } else {
                    missing.insert(bot.id);
                }
                continue;
            }
            let channel = self.channel(bot.id, conversation.channel_id)?;
            if !channel.enabled || channel.deleted {
                continue;
            }
            let Ok(instructions) = self.instructions(&bot) else {
                failed.push(*id);
                continue;
            };
            ready.push(Run {
                turn: self.turn(*id)?,
                owner: self.owner(&conversation)?,
                config: conversation.execution.clone(),
                conversation,
                instructions,
                cancel: CancellationToken::new(),
                output_cursor: None,
            });
        }
        for id in failed {
            self.progress(id, TurnStatus::Failed, None, Some("Workspace memory could not be read; repair the memory file before sending again".into()))?;
        }
        for bot in missing {
            self.pause_bot(bot, PauseReason::DirectoryMissing)?;
        }
        for run in &mut ready {
            run.turn.status = TurnStatus::Sending;
            self.commit_turn(&run.turn)?;
        }
        for run in &ready {
            self.running.insert(run.conversation.id, run.cancel.clone());
        }
        Ok(ready)
    }

    pub(crate) fn review_preparations(&mut self) -> Vec<ReviewPreparation> {
        let pending = self.pending.values().copied().collect::<Vec<_>>();
        let mut preparations = Vec::new();
        for id in pending {
            if preparations.len() >= 4 || self.preparing.contains(&id) {
                continue;
            }
            let Ok(turn) = self.turn(id) else {
                continue;
            };
            let Some(context) = receipt_context(&turn) else {
                continue;
            };
            let Some(conversation) = self.conversations.get(&turn.conversation_id) else {
                continue;
            };
            if conversation.execution_workspace_id.is_some() {
                continue;
            }
            self.preparing.insert(id);
            preparations.push(ReviewPreparation {
                turn_id: id,
                context,
            });
        }
        preparations
    }

    pub(crate) fn review_prepared(
        &mut self,
        turn_id: BotTurnId,
        workspace_id: Result<String, ()>,
    ) -> Result<(), ErrorCode> {
        self.preparing.remove(&turn_id);
        let turn = self.turn(turn_id)?;
        let mut conversation = self
            .conversations
            .get(&turn.conversation_id)
            .ok_or(ErrorCode::BotIo)?
            .clone();
        match workspace_id {
            Ok(workspace_id) => {
                self.active_workspace(&workspace_id)?;
                conversation.execution_workspace_id = Some(workspace_id);
                self.save_conversation(conversation)
            }
            Err(()) => self.progress(
                turn_id,
                TurnStatus::Failed,
                None,
                Some("The frozen pull-request checkout could not be prepared".to_owned()),
            ),
        }
    }
    fn instructions(&self, bot: &Bot) -> Result<String, ErrorCode> {
        let memory = self
            .store
            .memory(&bot.workspace_id)
            .map_err(|_| ErrorCode::BotIo)?;
        Ok(crate::instructions::compose(&memory, &bot.permissions))
    }
    pub(crate) fn progress(
        &mut self,
        id: BotTurnId,
        status: TurnStatus,
        reply: Option<String>,
        error: Option<String>,
    ) -> Result<(), ErrorCode> {
        self.progress_with_messages(id, status, reply, None, error)
    }

    pub(crate) fn progress_with_messages(
        &mut self,
        id: BotTurnId,
        status: TurnStatus,
        reply: Option<String>,
        messages: Option<Vec<TurnMessage>>,
        error: Option<String>,
    ) -> Result<(), ErrorCode> {
        let mut turn = self.turn(id)?;
        if matches!(
            turn.status,
            TurnStatus::Completed
                | TurnStatus::Failed
                | TurnStatus::Uncertain
                | TurnStatus::Cancelled
        ) {
            return Ok(());
        }
        let messages_changed = messages
            .as_ref()
            .is_some_and(|messages| turn.messages.as_slice() != messages.as_slice());
        if turn.status == status && turn.reply == reply && turn.error == error && !messages_changed
        {
            return Ok(());
        }
        turn.status = status;
        turn.reply = reply;
        turn.error = error;
        if let Some(messages) = messages {
            if messages.iter().any(|message| {
                message.id.is_empty()
                    || message.id.len() > 256
                    || message.text.len() > 256 * 1024
                    || message.text.contains('\0')
            }) {
                return Err(ErrorCode::InvalidMessage);
            }
            turn.messages = messages;
        }
        if status == TurnStatus::Completed {
            for message in &mut turn.messages {
                message.status = TurnMessageStatus::Completed;
            }
        }
        if status != TurnStatus::NeedsApproval {
            turn.pending_permissions.clear();
        }
        let conversation = self
            .conversations
            .get(&turn.conversation_id)
            .ok_or(ErrorCode::BotIo)?;
        if status == TurnStatus::Completed
            && matches!(
                self.channel(conversation.bot_id, conversation.channel_id)?
                    .config,
                ChannelConfig::Builtin
            )
        {
            turn.delivery = DeliveryStatus::Delivered;
        }
        self.commit_turn(&turn)
    }
    pub(crate) fn finished(&mut self, conversation: ConversationId) {
        self.running.remove(&conversation);
    }
    pub(crate) fn cancel(&mut self, request: &TurnRequest) -> Result<Value, ErrorCode> {
        let turn = self.owned_turn(self.current(&request.workspace_id)?.id, request.turn_id)?;
        if turn.status == TurnStatus::Queued {
            self.progress(turn.id, TurnStatus::Cancelled, None, None)?;
            return Ok(json!({"turn":self.turn(turn.id)?}));
        }
        if !matches!(
            turn.status,
            TurnStatus::Sending | TurnStatus::Running | TurnStatus::NeedsApproval
        ) {
            return Ok(json!({"turn":turn}));
        }
        if let Some(cancel) = self.running.get(&turn.conversation_id) {
            cancel.cancel();
            return Ok(json!({"cancellationRequested":true}));
        }
        Ok(json!({"turn":self.turn(turn.id)?}))
    }
    pub(crate) fn pending_delivery(
        &self,
        channel: domain::ChannelId,
    ) -> Result<Option<(Turn, domain::bot::Conversation)>, ErrorCode> {
        self.ensure_writable()?;
        let configured = self
            .catalog
            .channels
            .iter()
            .find(|entry| entry.id == channel)
            .ok_or(ErrorCode::BotNotFound)?;
        self.workspace(self.bot(configured.bot_id)?)?;
        if !configured.enabled || configured.deleted {
            return Err(ErrorCode::BotConflict);
        }
        self.outbox
            .get(&channel)
            .and_then(|outbox| outbox.first_key_value())
            .map(|(key, id)| {
                let conversation = self.conversations.get(&key.0).ok_or(ErrorCode::BotIo)?;
                Ok((self.turn(*id)?, conversation.clone()))
            })
            .transpose()
    }
    pub(crate) fn delivery(
        &mut self,
        id: BotTurnId,
        status: DeliveryStatus,
    ) -> Result<(), ErrorCode> {
        let mut turn = self.turn(id)?;
        turn.delivery = status;
        self.commit_turn(&turn)
    }
}

fn receipt_context(turn: &Turn) -> Option<GithubReviewContext> {
    turn.context
        .as_ref()
        .map(|TurnContext::GithubReview(context)| context.clone())
}
