use std::collections::BTreeSet;
use std::path::Path;

use domain::bot::{BotState, ChannelConfig, DeliveryStatus, PauseReason, TurnStatus};
use domain::{BotId, BotTurnId, ConversationId};
use model::ErrorCode;
use provider::service::bot_execution::Action;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::State;
use crate::execution::Run;
use crate::protocol::TurnRequest;

impl State {
    pub(crate) fn stop_bot(&mut self, bot: BotId, retire: bool) -> Result<(), ErrorCode> {
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
                self.progress(id, TurnStatus::Cancelled, None, None)?;
            }
        }
        Ok(())
    }
    /// Retire the archived Workspaces' Bots and delete those Workspaces' Bot memory.
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
        if !bots.is_empty() {
            self.retire_bots(&bots)?;
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
            let bot = self.bot(conversation.bot_id)?.clone();
            if missing.contains(&bot.id) {
                continue;
            }
            let Ok(cwd) = self.workspace(&bot) else {
                continue;
            };
            if !Path::new(&cwd).is_dir() {
                missing.insert(bot.id);
                continue;
            }
            let channel = self.channel(bot.id, conversation.channel_id)?;
            if !channel.enabled || channel.deleted {
                continue;
            }
            let Ok(instructions) = self.instructions(&bot.workspace_id) else {
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
    fn instructions(&self, workspace: &str) -> Result<String, ErrorCode> {
        let memory = self.store.memory(workspace).map_err(|_| ErrorCode::BotIo)?;
        Ok(crate::instructions::compose(&memory))
    }
    pub(crate) fn progress(
        &mut self,
        id: BotTurnId,
        status: TurnStatus,
        reply: Option<String>,
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
        if turn.status == status && turn.reply == reply && turn.error == error {
            return Ok(());
        }
        turn.status = status;
        turn.reply = reply;
        turn.error = error;
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

use domain::bot::Turn;
