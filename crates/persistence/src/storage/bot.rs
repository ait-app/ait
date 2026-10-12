//! Atomic Bot catalog, receipts and workspace-local memory.

use std::fs;
use std::path::{Path, PathBuf};

use domain::bot::{Catalog, Conversation, Turn};
use domain::{BotId, BotTurnId, ConversationId};
use model::bot::{StorageError, Store};
use serde::{Serialize, de::DeserializeOwned};

use crate::{File, single::Error};

const RECORD_LIMIT: u64 = 2 * 1024 * 1024;
const MEMORY_LIMIT: u64 = 32 * 1024;

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
    fn memory(&self, cwd: &str, workspace: &str, bot: BotId) -> Result<String, StorageError> {
        let path = memory_path(cwd, workspace, bot, false)?;
        if path
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(StorageError);
        }
        match File::new(path).read_limited(MEMORY_LIMIT) {
            Ok(bytes) => String::from_utf8(bytes).map_err(|_| StorageError),
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(String::new())
            }
            Err(Error::Io(_) | Error::TooLarge) => Err(StorageError),
        }
    }
    fn save_memory(
        &self,
        cwd: &str,
        workspace: &str,
        bot: BotId,
        text: &str,
    ) -> Result<(), StorageError> {
        if text.len() > usize::try_from(MEMORY_LIMIT).map_err(|_| StorageError)?
            || text.contains('\0')
        {
            return Err(StorageError);
        }
        File::new(memory_path(cwd, workspace, bot, true)?)
            .write(text.as_bytes())
            .map_err(|_| StorageError)
    }
}

fn memory_path(
    cwd: &str,
    workspace: &str,
    bot: BotId,
    create: bool,
) -> Result<PathBuf, StorageError> {
    let mut path = Path::new(cwd).canonicalize().map_err(|_| StorageError)?;
    let relative = model::bot::memory_path(workspace, bot);
    let parent = relative.parent().ok_or(StorageError)?;
    for component in parent.components() {
        path.push(component);
        if create && !path.exists() {
            fs::create_dir(&path).map_err(|_| StorageError)?;
        }
        match path.symlink_metadata() {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Err(error) if !create && error.kind() == std::io::ErrorKind::NotFound => {}
            Ok(_) | Err(_) => return Err(StorageError),
        }
    }
    path.push("MEMORY.md");
    Ok(path)
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
