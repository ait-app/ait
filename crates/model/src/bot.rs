//! Blocking storage and runtime-secret ports for the Bot application.

use domain::bot::{Catalog, Conversation, Turn};
use domain::{BotId, BotTurnId, ConversationId};
use sha2::{Digest, Sha256};

/// Relative workspace-local memory path, namespaced even when workspaces share one cwd.
#[must_use]
pub fn memory_path(workspace: &str, bot: BotId) -> std::path::PathBuf {
    std::path::PathBuf::from(".ait/bots")
        .join(format!("{:x}", Sha256::digest(workspace.as_bytes())))
        .join(bot.to_string())
        .join("MEMORY.md")
}

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
    /// Read bounded Bot memory from the owning workspace's directory.
    /// # Errors
    /// Returns unsafe paths, oversized memory, or inaccessible storage errors.
    fn memory(&self, cwd: &str, workspace: &str, bot: BotId) -> Result<String, StorageError>;
    /// Atomically replace bounded Bot memory in the workspace directory.
    /// # Errors
    /// Returns unsafe paths, oversized memory, or write failures.
    fn save_memory(
        &self,
        cwd: &str,
        workspace: &str,
        bot: BotId,
        text: &str,
    ) -> Result<(), StorageError>;
}
