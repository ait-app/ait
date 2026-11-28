use domain::BotId;
use domain::bot::Conversation;
use model::ErrorCode;
use serde_json::{Value, json};

use super::State;
use crate::channels::Incoming;
use crate::protocol::WorkspaceRequest;

impl State {
    /// Resolve the Bot's single durable native context, retaining external routes for delivery.
    pub(super) fn context(&mut self, bot: BotId) -> Result<Conversation, ErrorCode> {
        if let Some(id) = self.catalog.contexts.get(&bot) {
            return self.conversation(bot, *id).cloned();
        }
        let builtin = self.bot(bot)?.builtin_channel_id;
        let mut context = self.route(&Incoming {
            channel_id: builtin,
            peer: "owner".into(),
            sender: "owner".into(),
            thread: None,
            message_id: String::new(),
            text: String::new(),
            reply_to: None,
            sender_type: None,
            passive: false,
            context: None,
        })?;
        // The former owner runtime has an isolated history. Start a new native generation;
        // legacy transcripts remain available through bot_read rather than replaying work.
        let old_routes: Vec<_> = self
            .conversations
            .values()
            .filter(|c| c.bot_id == bot && c.id != context.id)
            .cloned()
            .collect();
        for route in old_routes {
            self.session_actions.push((
                self.owner(&route)?,
                provider::service::bot_execution::Action::Close {
                    agent_id: route.session_id,
                },
            ));
        }
        if self.receipts.values().any(|receipt| {
            receipt.conversation == context.id && receipt.status != domain::bot::TurnStatus::Queued
        }) {
            self.renew(&mut context)?;
            for id in self.queued(context.id) {
                let mut turn = self.turn(id)?;
                turn.generation = context.generation;
                self.commit_turn(&turn)?;
            }
        }
        let mut catalog = self.catalog.clone();
        catalog.contexts.insert(bot, context.id);
        self.save_catalog(catalog)?;
        Ok(context)
    }

    pub(super) fn initialize_contexts(&mut self) -> Result<(), ErrorCode> {
        let bot_ids: Vec<_> = self
            .catalog
            .bots
            .iter()
            .filter(|bot| {
                matches!(bot.state, domain::bot::BotState::Active)
                    && self.pending.keys().any(|(id, _)| {
                        self.conversations
                            .get(id)
                            .is_some_and(|c| c.bot_id == bot.id)
                    })
                    && !self.uncertain.iter().any(|(id, generation)| {
                        self.conversations
                            .get(id)
                            .is_some_and(|c| c.bot_id == bot.id && c.generation == *generation)
                    })
            })
            .map(|bot| bot.id)
            .collect();
        for bot in bot_ids {
            self.context(bot)?;
        }
        Ok(())
    }

    pub(super) fn ordered_pending(&self) -> Result<Vec<domain::bot::Turn>, ErrorCode> {
        let mut pending = self
            .pending
            .values()
            .map(|id| self.turn(*id))
            .collect::<Result<Vec<_>, _>>()?;
        pending.sort_by(|a, b| {
            a.bot_sequence
                .cmp(&b.bot_sequence)
                .then(a.created_at.cmp(&b.created_at))
                .then(a.id.cmp(&b.id))
        });
        Ok(pending)
    }

    pub(super) fn reset_context(&mut self, request: &WorkspaceRequest) -> Result<Value, ErrorCode> {
        let bot = self.current(&request.workspace_id)?.id;
        if self
            .running
            .keys()
            .any(|id| self.conversations.get(id).is_some_and(|c| c.bot_id == bot))
        {
            return Err(ErrorCode::BotConflict);
        }
        let routes: Vec<_> = self
            .conversations
            .values()
            .filter(|c| c.bot_id == bot)
            .map(|c| c.id)
            .collect();
        for id in routes {
            self.reset(&crate::protocol::ConversationRequest {
                workspace_id: request.workspace_id.clone(),
                conversation_id: id,
            })?;
        }
        if !self.catalog.contexts.contains_key(&bot) {
            let builtin = self.bot(bot)?.builtin_channel_id;
            let context_id = self
                .conversations
                .values()
                .find(|c| {
                    c.bot_id == bot
                        && c.channel_id == builtin
                        && c.peer == "owner"
                        && c.sender == "owner"
                })
                .map(|c| c.id);
            if let Some(id) = context_id {
                let mut catalog = self.catalog.clone();
                catalog.contexts.insert(bot, id);
                self.save_catalog(catalog)?;
            }
        }
        let context = self.context(bot)?;
        Ok(json!({"conversation":self.project_conversation(&context)}))
    }
}

#[cfg(test)]
mod tests;
