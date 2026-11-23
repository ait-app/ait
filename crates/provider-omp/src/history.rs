//! OMP's native history: session files read straight from disk, projected through the same
//! [`Stream`] as live output so replay and live rows agree (D17-D20, M6).

pub(super) mod children;
mod listing;
pub(super) mod paths;
mod reader;

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};
use serde_json::{Value, json};

use super::{LABEL, OmpClient, PROVIDER, config, stream::Stream};
use provider::local::session_preview;
use provider::ports::agent_session::{AgentSessionError, AgentTurnEvent};
use provider::ports::native_history::{ListOptions, SessionDescriptor, SessionHistory};

/// Bytes of session files read per listing; later files are not described.
const MAX_LISTING_BYTES: u64 = 256 * 1024 * 1024;

/// Entry types that carry no conversation: Paseo's control entries and the other control
/// entries OMP 18.18.8 writes (`service_tier_change`, `mode_change`, `model_usage`, `label`,
/// `credential_pin`, `reset_boundary`, `branch_summary`).
const CONTROL_ENTRIES: [&str; 18] = [
    "session",
    "session_init",
    "session_info",
    "system",
    "title",
    "title_change",
    "custom",
    "system_prompt",
    "model_change",
    "thinking_level_change",
    "ttsr_injection",
    "service_tier_change",
    "mode_change",
    "model_usage",
    "label",
    "credential_pin",
    "reset_boundary",
    "branch_summary",
];

/// One session's active branch, ready to project.
pub(super) struct Journal {
    pub(super) path: PathBuf,
    pub(super) header: Value,
    title: Option<String>,
    entries: Vec<Value>,
    /// The native identity rows are keyed by: the header's id, or a child's qualified id.
    identity: String,
    /// The immediate parent of a subagent transcript.
    parent: Option<String>,
}

/// A requested working directory, matched by spelling or by canonical path.
///
/// OMP records its physical working directory, so a symlinked workspace spelling still
/// identifies the same journals.
struct Workspace<'a> {
    spelling: &'a str,
    canonical: Option<PathBuf>,
}

impl<'a> Workspace<'a> {
    /// Resolve `spelling` once; this blocks on file-system I/O.
    fn new(spelling: &'a str) -> Self {
        Self {
            spelling,
            canonical: std::fs::canonicalize(spelling).ok(),
        }
    }

    /// Whether a journal's recorded `cwd` names this workspace; unresolvable paths
    /// match only by identical spelling.
    fn matches(&self, cwd: &Value) -> bool {
        cwd.as_str().is_some_and(|cwd| {
            cwd == self.spelling
                || self.canonical.as_ref().is_some_and(|canonical| {
                    std::fs::canonicalize(cwd).is_ok_and(|resolved| resolved == *canonical)
                })
        })
    }
}

/// Validated persistence lookup, captured before any blocking file-system work.
struct Lookup {
    known: Option<PathBuf>,
    root: PathBuf,
    id: String,
    cwd: String,
    empty: Option<String>,
}

impl Lookup {
    fn new(
        client: &OmpClient,
        handle: &AgentPersistenceHandle,
        cwd: &str,
    ) -> Result<Self, AgentSessionError> {
        if handle.provider != PROVIDER
            || uuid::Uuid::parse_str(&handle.session_id).is_err()
            || !Path::new(cwd).is_absolute()
        {
            return Err(AgentSessionError::Rejected);
        }
        let metadata = handle.metadata.as_ref();
        let empty = metadata
            .filter(|metadata| {
                metadata.get("empty") == Some(&json!(true))
                    && metadata.get("cwd") == Some(&json!(cwd))
            })
            .and_then(|metadata| metadata.get("createdAt"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        // Ait records the session file in its metadata; Paseo's handles name it natively.
        let known = metadata
            .and_then(|metadata| metadata.get("sessionFile"))
            .and_then(Value::as_str)
            .or_else(|| handle.native_handle.as_ref().and_then(Value::as_str))
            .map(PathBuf::from);
        Ok(Self {
            known,
            root: paths::sessions(client, Some(cwd))?,
            id: handle.session_id.clone(),
            cwd: cwd.to_owned(),
            empty,
        })
    }

    /// Locate and read the journal; this blocks on file-system I/O.
    fn load(self) -> Result<Journal, AgentSessionError> {
        let Self {
            known,
            root,
            id,
            cwd,
            empty,
        } = self;
        let path = match known {
            Some(path) if path.is_absolute() => path,
            Some(_) => return Err(AgentSessionError::Rejected),
            None => paths::scan(&root)?
                .into_iter()
                .find(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.ends_with(&format!("_{id}.jsonl")))
                })
                .ok_or(AgentSessionError::Unavailable)?,
        };
        if let Some(created) = empty
            && matches!(path.symlink_metadata(), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
        {
            chrono::DateTime::parse_from_rfc3339(&created)
                .map_err(|_| AgentSessionError::Rejected)?;
            return Ok(Journal {
                path,
                header: json!({"type":"session","version":3,"id":id,"cwd":cwd,"timestamp":created}),
                title: None,
                entries: Vec::new(),
                identity: id,
                parent: None,
            });
        }
        let snapshot = reader::read(&path)?;
        if snapshot.header["id"] != id.as_str()
            || !Workspace::new(&cwd).matches(&snapshot.header["cwd"])
        {
            return Err(AgentSessionError::Rejected);
        }
        Ok(Journal {
            path,
            header: snapshot.header,
            title: snapshot.title,
            entries: snapshot.entries,
            identity: id,
            parent: None,
        })
    }
}

/// Load a resumable journal and, when it is persisted, replay it for the live stream.
/// Reading, branch projection and image materialization all run off the async runtime.
/// Returns a rejected foreign handle, a missing journal, or an unreadable session file.
pub(super) async fn resume(
    client: &OmpClient,
    handle: &AgentPersistenceHandle,
    cwd: &str,
) -> Result<(Journal, Option<Stream>), AgentSessionError> {
    let lookup = Lookup::new(client, handle, cwd)?;
    let client = client.clone();
    let handle = handle.clone();
    tokio::task::spawn_blocking(move || {
        let journal = lookup.load()?;
        let stream = if journal.path.is_file() {
            Some(journal.project(&client, &handle)?.1)
        } else {
            None
        };
        Ok((journal, stream))
    })
    .await
    .map_err(|_| AgentSessionError::Failed)?
}

/// A top-level `custom_message` entry as the custom message OMP announced live, stamped with
/// the entry's time; none without a valid time.
fn record_message(record: &Value) -> Option<Value> {
    Some(json!({"role":"custom","timestamp":entry_time(record)?,
        "customType":record["customType"],"content":record["content"],
        "display":record["display"],"details":record["details"],
        "attribution":record["attribution"]}))
}

/// An entry's ISO `timestamp` in epoch milliseconds.
fn entry_time(entry: &Value) -> Option<i64> {
    entry["timestamp"]
        .as_str()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|time| time.timestamp_millis())
}

/// An unknown entry as Paseo's `[type] text` custom row; none without a valid time.
fn unknown_entry(entry: &Value, kind: &str) -> Option<Value> {
    Some(
        json!({"role":"custom","timestamp":entry_time(entry)?,"display":true,
        "content":super::stream::fallback_text(kind, entry)}),
    )
}

/// Model and effort recorded on the branch, and whether it ends mid-turn.
#[derive(Default)]
struct Replay {
    config: StoredAgentConfig,
    active: bool,
    entries: Vec<provider::protocol::timeline::NativeItem>,
}

impl Journal {
    /// Summarize the branch: a native title, else a readable file name; its user prompts;
    /// and its last entry's time.
    fn describe(&self) -> Result<SessionDescriptor, AgentSessionError> {
        let mut updated = self.header["timestamp"]
            .as_str()
            .ok_or(AgentSessionError::Failed)?;
        let mut first_prompt = None;
        let mut last_prompt = None;
        for entry in &self.entries {
            if let Some(timestamp) = entry["timestamp"].as_str() {
                updated = timestamp;
            }
            if entry["type"] == "message"
                && entry["message"]["role"] == "user"
                && let Some(preview) = prompt_preview(&entry["message"]["content"])
            {
                first_prompt.get_or_insert_with(|| preview.clone());
                last_prompt = Some(preview);
            }
        }
        Ok(SessionDescriptor {
            provider_id: PROVIDER.to_owned(),
            provider_label: LABEL.to_owned(),
            provider_handle_id: self.identity.clone(),
            cwd: config::text(&self.header, "cwd")?.to_owned(),
            title: self
                .title
                .clone()
                .or_else(|| listing::readable_name(&self.path)),
            first_prompt_preview: first_prompt,
            last_prompt_preview: last_prompt,
            last_activity_at: updated.to_owned(),
        })
    }

    /// Replay the active branch into timeline entries and a stream ready for live events.
    /// An entry that cannot be projected is skipped, and an image that can no longer be
    /// materialized degrades to a placeholder. This blocks on image file-system I/O.
    ///
    /// # Errors
    ///
    /// Returns failed for a header without a creation time or working directory, and
    /// unavailable or rejected when OMP's storage cannot be resolved.
    pub(super) fn project(
        &self,
        client: &OmpClient,
        handle: &AgentPersistenceHandle,
    ) -> Result<(SessionHistory, Stream), AgentSessionError> {
        let mut stream = Stream::new(self.identity.clone(), client.images.clone())
            .with_blobs(paths::blobs(client, self.header["cwd"].as_str())?);
        restore_input_ids(&mut stream, handle);
        stream.replay_images(true);
        // A call without a result on the branch is still running (D18).
        let results: HashSet<&str> = self
            .entries
            .iter()
            .filter(|entry| entry["message"]["role"] == "toolResult")
            .filter_map(|entry| entry["message"]["toolCallId"].as_str())
            .collect();
        let mut replay = Replay::default();
        for entry in &self.entries {
            replay_entry(&mut stream, entry, &results, &mut replay);
        }
        stream.replay_images(false);
        let descriptor = self.describe()?;
        let resume_metadata = BTreeMap::from([
            ("cwd".to_owned(), self.header["cwd"].clone()),
            ("sessionFile".to_owned(), json!(self.path)),
            ("inputIds".to_owned(), json!(stream.input_ids)),
        ]);
        let created_at = self.header["timestamp"]
            .as_str()
            .ok_or(AgentSessionError::Failed)?
            .to_owned();
        Ok((
            SessionHistory {
                resume_metadata,
                parent_id: self.parent.clone(),
                descriptor,
                created_at,
                config: replay.config,
                active: replay.active,
                entries: replay.entries,
            },
            stream,
        ))
    }
}

/// Project one branch entry as Paseo's `streamOmpHistory` maps it (D18).
fn replay_entry(stream: &mut Stream, entry: &Value, results: &HashSet<&str>, replay: &mut Replay) {
    let id = entry["id"].as_str().unwrap_or_default();
    let projected = match entry["type"].as_str() {
        Some("message") => {
            let message = &entry["message"];
            match message["role"].as_str() {
                Some("user") => replay.active = true,
                Some("assistant") => {
                    replay.active = message["stopReason"] == "toolUse";
                    if let (Some(provider), Some(model)) =
                        (message["provider"].as_str(), message["model"].as_str())
                    {
                        replay.config.model = Some(json!([provider, model]).to_string());
                    }
                }
                _ => {}
            }
            // A user message is named by its native entry, as live output is (D9).
            stream.replayed(message, id).map(|()| {
                if message["role"] == "assistant" {
                    stream.running_calls(message, results);
                }
            })
        }
        Some("custom_message") => match record_message(entry) {
            Some(message) => stream.replayed(&message, id),
            None => Ok(()),
        },
        Some("compaction") => {
            stream.compaction(entry);
            Ok(())
        }
        Some("model_change") => {
            if entry["role"].as_str().is_none_or(|role| role == "default")
                && let Some((provider, model)) = entry["model"]
                    .as_str()
                    .and_then(|model| model.split_once('/'))
            {
                replay.config.model = Some(json!([provider, model]).to_string());
            }
            Ok(())
        }
        Some("thinking_level_change") => {
            replay.config.thinking_option_id = entry["thinkingLevel"].as_str().map(str::to_owned);
            Ok(())
        }
        Some(kind) if CONTROL_ENTRIES.contains(&kind) || kind.starts_with("tool_execution") => {
            Ok(())
        }
        Some(kind) => match unknown_entry(entry, kind) {
            Some(message) => stream.replayed(&message, id),
            None => Ok(()),
        },
        None => Ok(()),
    };
    if projected.is_err() {
        // O13: one entry Ait cannot project does not hide the rest of the history.
        tracing::debug!(
            provider = LABEL,
            "Skipping an OMP history entry that cannot be shown"
        );
        stream.events.clear();
        return;
    }
    replay
        .entries
        .extend(stream.events.drain(..).filter_map(|event| match event {
            AgentTurnEvent::Timeline(entry) => Some(entry),
            _ => None,
        }));
}

/// A whitespace-normalized preview of a user message's text.
fn prompt_preview(content: &Value) -> Option<String> {
    match content {
        Value::String(text) => session_preview::text(std::iter::once(text.as_str())),
        Value::Array(blocks) => session_preview::text(
            blocks
                .iter()
                .filter(|block| block["type"] == "text")
                .filter_map(|block| block["text"].as_str()),
        ),
        _ => None,
    }
}

/// Read and project the history `handle` names: a root session, or a subagent transcript
/// located by its handle (M6).
pub(super) async fn inspect(
    client: &OmpClient,
    handle: &AgentPersistenceHandle,
    cwd: &str,
) -> Result<SessionHistory, AgentSessionError> {
    let client = client.clone();
    let handle = handle.clone();
    if children::is_child(&handle) {
        let cwd = cwd.to_owned();
        return tokio::task::spawn_blocking(move || children::read(&client, &handle, &cwd))
            .await
            .map_err(|_| AgentSessionError::Failed)?;
    }
    let lookup = Lookup::new(&client, &handle, cwd)?;
    tokio::task::spawn_blocking(move || {
        lookup
            .load()?
            .project(&client, &handle)
            .map(|(history, _)| history)
    })
    .await
    .map_err(|_| AgentSessionError::Failed)?
}

/// Discover the children recorded for workspace `cwd` (M6).
pub(super) async fn subagents(
    client: &OmpClient,
    cwd: &str,
) -> Result<Vec<provider::ports::controls::NativeSubagent>, AgentSessionError> {
    let (client, cwd) = (client.clone(), cwd.to_owned());
    tokio::task::spawn_blocking(move || children::list(&client, &cwd))
        .await
        .map_err(|_| AgentSessionError::Failed)?
}

/// List sessions newest first from each file's head and tail (D19), inspecting at most
/// `scan_limit` matching sessions.
pub(super) async fn list(
    client: &OmpClient,
    options: &ListOptions,
) -> Result<Vec<SessionDescriptor>, AgentSessionError> {
    if !(1..=4096).contains(&options.scan_limit) {
        return Err(AgentSessionError::Rejected);
    }
    let root = paths::sessions(client, options.cwd.as_deref())?;
    let options = options.clone();
    tokio::task::spawn_blocking(move || {
        let workspace = options.cwd.as_deref().map(Workspace::new);
        let matches = |cwd: &Value| {
            workspace
                .as_ref()
                .is_none_or(|workspace| workspace.matches(cwd))
        };
        let mut sessions = Vec::new();
        let mut bytes = 0;
        for path in paths::scan(&root)? {
            // A native writer can remove or replace a candidate after the directory scan.
            let Some(listed) = listing::describe(&path, matches) else {
                continue;
            };
            bytes += listed.bytes;
            sessions.extend(listed.descriptor);
            if sessions.len() >= options.scan_limit || bytes > MAX_LISTING_BYTES {
                break;
            }
        }
        sessions.sort_by(|left, right| right.last_activity_at.cmp(&left.last_activity_at));
        Ok(sessions)
    })
    .await
    .map_err(|_| AgentSessionError::Failed)?
}

/// Restore the client identities `handle` recorded for its user messages.
fn restore_input_ids(stream: &mut Stream, handle: &AgentPersistenceHandle) {
    if let Some(values) = handle
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("inputIds"))
        .and_then(Value::as_object)
    {
        stream.input_ids.extend(
            values
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
        // Handles written before the map was bounded may hold more; the newest are kept.
        super::stream::retain_newest_inputs(&mut stream.input_ids);
    }
}

#[cfg(test)]
mod tests;
