//! Blocking storage and runtime-secret ports for the Bot application.

use domain::bot::notification::NotificationLog;
use domain::bot::ticket::Ticket;
use domain::bot::{Catalog, Conversation, Turn};
use domain::{BotId, BotTurnId, ConversationId, TicketId};

/// A safe Bot persistence failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Bot storage failed")]
pub struct StorageError;

/// Host-owned, atomic local storage, invoked away from async reactors.
pub trait Store: std::fmt::Debug + Send + Sync {
    /// Load the configuration catalog, returning an empty catalog on first use.
    /// # Errors
    /// Returns malformed or inaccessible storage errors.
    fn catalog(&self) -> Result<Catalog, StorageError>;
    /// Atomically commit the full configuration catalog.
    /// # Errors
    /// Returns write or sync failures; callers must stop on an uncertain write.
    fn save_catalog(&self, catalog: &Catalog) -> Result<(), StorageError>;
    /// Load all durable conversation routes.
    /// # Errors
    /// Returns malformed or inaccessible storage errors.
    fn conversations(&self) -> Result<Vec<Conversation>, StorageError>;
    /// Atomically commit one conversation.
    /// # Errors
    /// Returns write or sync failures.
    fn save_conversation(&self, conversation: &Conversation) -> Result<(), StorageError>;
    /// Remove one conversation and its receipts; an absent conversation is not an error.
    /// The route goes first, so an interrupted removal leaves only unreachable receipts.
    /// # Errors
    /// Returns inaccessible storage errors.
    fn delete_conversation(&self, id: ConversationId) -> Result<(), StorageError>;
    /// Load the durable receipts belonging to one conversation.
    /// # Errors
    /// Returns malformed or inaccessible storage errors.
    fn turns(&self, id: ConversationId) -> Result<Vec<Turn>, StorageError>;
    /// Visit one receipt at a time during startup without retaining complete history text.
    /// # Errors
    /// Returns record I/O failures or a rejected invariant from the visitor.
    fn scan_turns(
        &self,
        id: ConversationId,
        visitor: &mut dyn FnMut(Turn) -> Result<(), StorageError>,
    ) -> Result<(), StorageError>;
    /// Read one durable receipt without loading its conversation history.
    /// # Errors
    /// Returns absent, malformed or inaccessible storage errors.
    fn turn(&self, conversation: ConversationId, id: BotTurnId) -> Result<Turn, StorageError>;
    /// Atomically commit one receipt without rewriting the history.
    /// # Errors
    /// Returns write or sync failures.
    fn save_turn(&self, turn: &Turn) -> Result<(), StorageError>;
    /// Read the bounded memory of one workspace's Bot; absent memory reads as empty.
    /// # Errors
    /// Returns oversized memory, invalid text, or inaccessible storage errors.
    fn memory(&self, workspace: &str) -> Result<String, StorageError>;
    /// Atomically replace the bounded memory of one workspace's Bot.
    /// # Errors
    /// Returns oversized or NUL-containing memory, or write failures.
    fn save_memory(&self, workspace: &str, text: &str) -> Result<(), StorageError>;
    /// Remove the memory of one workspace's Bot; absent memory is not an error.
    /// # Errors
    /// Returns inaccessible storage errors.
    fn delete_memory(&self, workspace: &str) -> Result<(), StorageError>;
    /// Load every Bot's notification log.
    /// # Errors
    /// Returns malformed or inaccessible storage errors.
    fn notification_logs(&self) -> Result<Vec<NotificationLog>, StorageError>;
    /// Atomically replace one Bot's notification log.
    /// # Errors
    /// Returns oversized records or write failures; callers must stop on an uncertain write.
    fn save_notification_log(&self, log: &NotificationLog) -> Result<(), StorageError>;
    /// Remove one Bot's notification log; an absent log is not an error.
    /// # Errors
    /// Returns inaccessible storage errors.
    fn delete_notification_log(&self, bot: BotId) -> Result<(), StorageError>;
    /// Visit every stored ticket of every Bot one at a time during startup.
    /// # Errors
    /// Returns record I/O failures or a rejected invariant from the visitor.
    fn scan_tickets(
        &self,
        visitor: &mut dyn FnMut(Ticket) -> Result<(), StorageError>,
    ) -> Result<(), StorageError>;
    /// Read one ticket of `bot` with its reports.
    /// # Errors
    /// Returns absent, malformed or inaccessible storage errors.
    fn ticket(&self, bot: BotId, id: TicketId) -> Result<Ticket, StorageError>;
    /// Atomically replace one ticket.
    /// # Errors
    /// Returns oversized records or write failures; callers must stop on an uncertain write.
    fn save_ticket(&self, ticket: &Ticket) -> Result<(), StorageError>;
    /// Remove one ticket of `bot`; an absent ticket is not an error.
    /// # Errors
    /// Returns inaccessible storage errors.
    fn delete_ticket(&self, bot: BotId, id: TicketId) -> Result<(), StorageError>;
    /// Remove every ticket of `bot`; absent tickets are not an error.
    /// # Errors
    /// Returns inaccessible storage errors.
    fn delete_tickets(&self, bot: BotId) -> Result<(), StorageError>;
}
