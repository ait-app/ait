//! Atomic Bot catalog, receipts and workspace-keyed memory.

use std::fs;
use std::path::{Path, PathBuf};

use domain::bot::{Catalog, Conversation, Turn};
use domain::{BotTurnId, ChannelId, ConversationId};
use model::bot::{StorageError, Store};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};

use crate::{File, single::Error};

const RECORD_LIMIT: u64 = 2 * 1024 * 1024;
const MEMORY_LIMIT: u64 = 32 * 1024;
const SECRET_LIMIT: u64 = 4096;

/// Concrete local storage installed only by the daemon composition root.
#[derive(Debug, Clone)]
pub struct FileStore {
    root: PathBuf,
}

impl FileStore {
    /// Select a daemon-owned directory; no I/O is performed until the store is used.
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

/// Owner-only secret files, one per channel, installed by the daemon composition root.
#[derive(Debug, Clone)]
pub struct SecretFiles {
    root: PathBuf,
}

impl SecretFiles {
    /// Select a daemon-owned directory; no I/O is performed until a secret is used.
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn path(&self, channel: ChannelId) -> PathBuf {
        self.root.join(channel.to_string())
    }

    /// Atomically replace `channel`'s secret; the file and directory are owner-only on Unix.
    /// # Errors
    /// Returns a safe storage error without echoing the secret.
    pub fn save(&self, channel: ChannelId, secret: &str) -> Result<(), StorageError> {
        if u64::try_from(secret.len()).map_or(true, |length| length > SECRET_LIMIT) {
            return Err(StorageError);
        }
        fs::create_dir_all(&self.root).map_err(|_| StorageError)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700))
                .map_err(|_| StorageError)?;
        }
        File::new(self.path(channel))
            .write(secret.as_bytes())
            .map_err(|_| StorageError)
    }

    /// Read `channel`'s secret.
    /// # Errors
    /// Returns a safe storage error when the secret is absent, oversized or unreadable.
    pub fn load(&self, channel: ChannelId) -> Result<String, StorageError> {
        let bytes = File::new(self.path(channel))
            .read_limited(SECRET_LIMIT)
            .map_err(|_| StorageError)?;
        String::from_utf8(bytes).map_err(|_| StorageError)
    }

    /// Remove `channel`'s secret; an absent secret is not an error.
    /// # Errors
    /// Returns a safe storage error when an existing secret cannot be removed.
    pub fn delete(&self, channel: ChannelId) -> Result<(), StorageError> {
        match fs::remove_file(self.path(channel)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(StorageError),
        }
    }
}

impl Store for FileStore {
    fn catalog(&self) -> Result<Catalog, StorageError> {
        read(&self.root.join("catalog.json")).map(Option::unwrap_or_default)
    }
    fn save_catalog(&self, catalog: &Catalog) -> Result<(), StorageError> {
        write(&self.root.join("catalog.json"), catalog)
    }
    fn conversations(&self) -> Result<Vec<Conversation>, StorageError> {
        records(&self.root.join("conversations"))
    }
    fn save_conversation(&self, conversation: &Conversation) -> Result<(), StorageError> {
        write(
            &self
                .root
                .join("conversations")
                .join(format!("{}.json", conversation.id)),
            conversation,
        )
    }
    fn delete_conversation(&self, id: ConversationId) -> Result<(), StorageError> {
        let absent = |result: std::io::Result<()>| match result {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(StorageError),
            _ => Ok(()),
        };
        absent(fs::remove_file(
            self.root.join("conversations").join(format!("{id}.json")),
        ))?;
        absent(fs::remove_dir_all(
            self.root.join("turns").join(id.to_string()),
        ))
    }
    fn turns(&self, id: ConversationId) -> Result<Vec<Turn>, StorageError> {
        let mut turns = Vec::new();
        self.scan_turns(id, &mut |turn| {
            turns.push(turn);
            Ok(())
        })?;
        Ok(turns)
    }
    fn scan_turns(
        &self,
        id: ConversationId,
        visitor: &mut dyn FnMut(Turn) -> Result<(), StorageError>,
    ) -> Result<(), StorageError> {
        visit_records(&self.root.join("turns").join(id.to_string()), visitor)
    }
    fn turn(&self, conversation: ConversationId, id: BotTurnId) -> Result<Turn, StorageError> {
        read(
            &self
                .root
                .join("turns")
                .join(conversation.to_string())
                .join(format!("{id}.json")),
        )?
        .ok_or(StorageError)
    }
    fn save_turn(&self, turn: &Turn) -> Result<(), StorageError> {
        write(
            &self
                .root
                .join("turns")
                .join(turn.conversation_id.to_string())
                .join(format!("{}.json", turn.id)),
            turn,
        )
    }
    fn memory(&self, workspace: &str) -> Result<String, StorageError> {
        match File::new(self.memory_path(workspace)).read_limited(MEMORY_LIMIT) {
            Ok(bytes) => String::from_utf8(bytes).map_err(|_| StorageError),
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(String::new())
            }
            Err(Error::Io(_) | Error::TooLarge) => Err(StorageError),
        }
    }
    fn save_memory(&self, workspace: &str, text: &str) -> Result<(), StorageError> {
        if text.len() > usize::try_from(MEMORY_LIMIT).map_err(|_| StorageError)?
            || text.contains('\0')
        {
            return Err(StorageError);
        }
        File::new(self.memory_path(workspace))
            .write(text.as_bytes())
            .map_err(|_| StorageError)
    }
    fn delete_memory(&self, workspace: &str) -> Result<(), StorageError> {
        match fs::remove_file(self.memory_path(workspace)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(StorageError),
        }
    }
}

impl FileStore {
    fn memory_path(&self, workspace: &str) -> PathBuf {
        self.root
            .join("memory")
            .join(format!("{:x}.md", Sha256::digest(workspace.as_bytes())))
    }
}

fn read<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, StorageError> {
    match File::new(path).read_limited(RECORD_LIMIT) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| StorageError),
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(Error::Io(_) | Error::TooLarge) => Err(StorageError),
    }
}

fn write<T: Serialize>(path: &Path, value: &T) -> Result<(), StorageError> {
    let bytes = serde_json::to_vec(value).map_err(|_| StorageError)?;
    if bytes.len() > usize::try_from(RECORD_LIMIT).map_err(|_| StorageError)? {
        return Err(StorageError);
    }
    File::new(path).write(&bytes).map_err(|_| StorageError)
}

fn records<T: DeserializeOwned>(directory: &Path) -> Result<Vec<T>, StorageError> {
    let mut records = Vec::new();
    visit_records(directory, &mut |record| {
        records.push(record);
        Ok(())
    })?;
    Ok(records)
}

fn visit_records<T: DeserializeOwned>(
    directory: &Path,
    visitor: &mut dyn FnMut(T) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(StorageError),
    };
    entries
        .filter_map(|entry| match entry {
            Ok(entry)
                if entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "json") =>
            {
                Some(Ok(entry))
            }
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .try_for_each(|entry| {
            let path = entry.map_err(|_| StorageError)?.path();
            visitor(read(&path)?.ok_or(StorageError)?)
        })
}

#[cfg(test)]
mod tests;
