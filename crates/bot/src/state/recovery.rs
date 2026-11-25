use std::collections::BTreeMap;

use domain::{BotId, ConversationId};
use model::ErrorCode;

use super::State;

impl State {
    /// Retain uncertain receipts while moving their Bot to a fresh native context on startup.
    ///
    /// # Errors
    /// Returns invalid-record, persistence or exhausted-generation errors. Admission must not
    /// open after an incomplete recovery.
    pub(super) fn recover_contexts(&mut self) -> Result<(), ErrorCode> {
        let mut affected: BTreeMap<BotId, Vec<ConversationId>> = BTreeMap::new();
        for (id, generation) in &self.uncertain {
            let conversation = self.conversations.get(id).ok_or(ErrorCode::BotIo)?;
            if conversation.generation == *generation {
                affected.entry(conversation.bot_id).or_default().push(*id);
            }
        }
        for (bot, sources) in affected {
            // Persist the new shared session before clearing any source's current uncertainty.
            // A crash between these writes can rotate again, but cannot resume the old session.
            let context = self.catalog.contexts.get(&bot).copied();
            if let Some(id) = context {
                let mut conversation = self.conversation(bot, id)?.clone();
                self.renew(&mut conversation)?;
            }
            for id in sources {
                if Some(id) != context {
                    let mut conversation = self.conversation(bot, id)?.clone();
                    self.renew(&mut conversation)?;
                }
            }
        }
        // Queued inputs have never reached a Provider. Reconcile them even when a preceding
        // startup stopped after saving a generation but before moving all its queued receipts.
        let queued: Vec<_> = self.pending.values().copied().collect();
        for id in queued {
            let mut turn = self.turn(id)?;
            let conversation = self
                .conversations
                .get(&turn.conversation_id)
                .ok_or(ErrorCode::BotIo)?;
            if turn.generation > conversation.generation {
                return Err(ErrorCode::BotIo);
            }
            if turn.generation < conversation.generation {
                turn.generation = conversation.generation;
                self.commit_turn(&turn)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
