use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};
use serde_json::{Value, json};

use super::{OmpClient, config, stream::Stream};
use crate::ports::agent_session::{AgentSessionError, AgentTurnEvent};
use crate::ports::native_history::{ListOptions, SessionDescriptor, SessionHistory};

const MAX_BYTES: u64 = 64 * 1024 * 1024;

pub(super) struct Journal {
    pub(super) path: PathBuf,
    pub(super) header: Value,
    records: Vec<Value>,
}

fn environment(client: &OmpClient, key: &str) -> Option<String> {
    client
        .environment
        .entries()
        .find_map(|(name, value)| (name == key).then(|| value.to_owned()))
        .or_else(|| std::env::var(key).ok())
}

pub(super) fn root(client: &OmpClient) -> Result<PathBuf, AgentSessionError> {
    let home = environment(client, "HOME")
        .or_else(|| environment(client, "USERPROFILE"))
        .map(PathBuf::from)
        .ok_or(AgentSessionError::Unavailable)?;
    let profile = environment(client, "OMP_PROFILE")
        .or_else(|| environment(client, "PI_PROFILE"))
        .filter(|profile| !profile.is_empty());
    let mut config =
        home.join(environment(client, "PI_CONFIG_DIR").unwrap_or_else(|| ".omp".to_owned()));
    if let Some(profile) = &profile {
        if !profile
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(AgentSessionError::Rejected);
        }
        config = config.join("profiles").join(profile);
    }
    if profile.is_none()
        && let Some(directory) = environment(client, "PI_CODING_AGENT_DIR")
    {
        let agent = directory
            .strip_prefix("~/")
            .map_or_else(|| PathBuf::from(&directory), |relative| home.join(relative));
        return Ok(agent.join("sessions"));
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if let Some(data) = environment(client, "XDG_DATA_HOME") {
        let mut data = PathBuf::from(data).join("omp");
        if let Some(profile) = &profile {
            data = data.join("profiles").join(profile);
        }
        if data.is_dir() {
            return Ok(data.join("sessions"));
        }
    }
    Ok(config.join("agent/sessions"))
}

fn paths(root: &Path) -> Result<Vec<PathBuf>, AgentSessionError> {
    let mut files = Vec::new();
    let mut directories = vec![root.to_path_buf()];
    let mut scanned = 0;
    while let Some(directory) = directories.pop() {
        let entries = match std::fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(AgentSessionError::Failed),
        };
        for entry in entries {
            scanned += 1;
            if scanned > 32768 {
                return Err(AgentSessionError::Failed);
            }
            let entry = entry.map_err(|_| AgentSessionError::Failed)?;
            let kind = entry.file_type().map_err(|_| AgentSessionError::Failed)?;
            if kind.is_dir() && entry.path().parent() == Some(root) {
                directories.push(entry.path());
            } else if kind.is_file()
                && entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "jsonl")
            {
                files.push(entry.path());
            }
        }
    }
    files.sort_by_cached_key(|path| {
        std::cmp::Reverse(
            std::fs::metadata(path)
                .and_then(|meta| meta.modified())
                .ok(),
        )
    });
    Ok(files)
}

fn read(path: PathBuf) -> Result<Journal, AgentSessionError> {
    let metadata = path
        .symlink_metadata()
        .map_err(|_| AgentSessionError::Unavailable)?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES {
        return Err(AgentSessionError::Failed);
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&path)
        .map_err(|_| AgentSessionError::Failed)?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| AgentSessionError::Failed)?;
    if bytes.len() as u64 > MAX_BYTES || bytes.last() != Some(&b'\n') {
        return Err(AgentSessionError::Failed);
    }
    let raw = bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice::<Value>(line).map_err(|_| AgentSessionError::Failed))
        .collect::<Result<Vec<_>, _>>()?;
    let title = raw
        .iter()
        .find(|record| record["type"] == "title")
        .and_then(|record| record["title"].as_str())
        .map(str::to_owned);
    let mut records = raw
        .into_iter()
        .filter(|record| record["type"] != "title")
        .map(Ok::<_, AgentSessionError>);
    let mut header = records.next().ok_or(AgentSessionError::Failed)??;
    if let Some(title) = title {
        header["title"] = json!(title);
    }
    if header["type"] != "session" || header["version"] != 3 {
        return Err(AgentSessionError::Failed);
    }
    config::text(&header, "id")?;
    config::text(&header, "cwd")?;
    let records = records.collect::<Result<Vec<_>, _>>()?;
    if records.len() > 131_072 {
        return Err(AgentSessionError::Failed);
    }
    Ok(Journal {
        path,
        header,
        records,
    })
}

pub(super) async fn load(
    client: &OmpClient,
    handle: &AgentPersistenceHandle,
    cwd: &str,
) -> Result<Journal, AgentSessionError> {
    if handle.provider != "omp"
        || uuid::Uuid::parse_str(&handle.session_id).is_err()
        || !Path::new(cwd).is_absolute()
    {
        return Err(AgentSessionError::Rejected);
    }
    let known = handle
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("sessionFile"))
        .and_then(Value::as_str)
        .map(PathBuf::from);
    let root = root(client)?;
    let id = handle.session_id.clone();
    let cwd = cwd.to_owned();
    let empty = handle
        .metadata
        .as_ref()
        .filter(|metadata| {
            metadata.get("empty") == Some(&json!(true)) && metadata.get("cwd") == Some(&json!(cwd))
        })
        .and_then(|metadata| metadata.get("createdAt"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    tokio::task::spawn_blocking(move || {
        let path = match known {
            Some(path) if path.is_absolute() => path,
            Some(_) => return Err(AgentSessionError::Rejected),
            None => paths(&root)?
                .into_iter()
                .find(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.ends_with(&format!("_{id}.jsonl")))
                })
                .or_else(|| empty.as_ref().map(|_| root.join(format!("ait-empty_{id}.jsonl"))))
                .ok_or(AgentSessionError::Unavailable)?,
        };
        if let Some(created) = empty
            && matches!(path.symlink_metadata(), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
        {
            chrono::DateTime::parse_from_rfc3339(&created).map_err(|_| AgentSessionError::Rejected)?;
            return Ok(Journal {path, header:json!({"type":"session","version":3,"id":id,"cwd":cwd,"timestamp":created}), records:Vec::new()});
        }
        let journal = read(path)?;
        if journal.header["id"] != id || journal.header["cwd"] != cwd {
            return Err(AgentSessionError::Rejected);
        }
        Ok(journal)
    })
    .await
    .map_err(|_| AgentSessionError::Failed)?
}

impl Journal {
    fn branch(&self) -> Result<Vec<&Value>, AgentSessionError> {
        let mut index = BTreeMap::new();
        for record in &self.records {
            let id = config::text(record, "id")?;
            if index.insert(id, record).is_some() {
                return Err(AgentSessionError::Failed);
            }
        }
        let mut path = Vec::new();
        let mut next = match self.records.last() {
            Some(record) if record.get("activeLeafId").is_some() => {
                match record["activeLeafId"].as_str() {
                    Some(id) => Some(*index.get(id).ok_or(AgentSessionError::Failed)?),
                    None => None,
                }
            }
            last => last,
        };
        let mut visited = BTreeSet::new();
        while let Some(record) = next {
            let id = config::text(record, "id")?;
            if !visited.insert(id) {
                return Err(AgentSessionError::Failed);
            }
            path.push(record);
            next = match record.get("parentId") {
                Some(Value::Null) => None,
                Some(Value::String(parent)) => Some(
                    *index
                        .get(parent.as_str())
                        .ok_or(AgentSessionError::Failed)?,
                ),
                _ => return Err(AgentSessionError::Failed),
            };
        }
        path.reverse();
        Ok(path)
    }

    fn resume_metadata(
        &self,
        handle: &AgentPersistenceHandle,
        stream: &Stream,
        config: &mut StoredAgentConfig,
    ) -> BTreeMap<String, Value> {
        let mut metadata = BTreeMap::from([
            ("cwd".to_owned(), self.header["cwd"].clone()),
            ("sessionFile".to_owned(), json!(self.path)),
            ("inputIds".to_owned(), json!(stream.input_ids)),
        ]);
        if handle
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("transport"))
            .and_then(Value::as_str)
            == Some("acp")
        {
            config.feature_values = Some(BTreeMap::from([("transport".to_owned(), json!("acp"))]));
            metadata.insert("transport".to_owned(), json!("acp"));
        }
        metadata
    }

    pub(super) fn project(
        &self,
        client: &OmpClient,
        handle: &AgentPersistenceHandle,
    ) -> Result<(SessionHistory, Stream), AgentSessionError> {
        if self
            .records
            .iter()
            .any(|record| record["type"] == "session_init")
        {
            return Err(AgentSessionError::Rejected);
        }
        let id = config::text(&self.header, "id")?;
        let mut stream = Stream::new(id.to_owned(), client.images.clone());
        restore_input_ids(&mut stream, handle)?;
        let mut config = StoredAgentConfig::default();
        let mut title = self.header["title"].as_str().map(str::to_owned);
        let mut active = false;
        let mut updated = self.header["timestamp"]
            .as_str()
            .ok_or(AgentSessionError::Failed)?
            .to_owned();
        let mut messages = Vec::new();
        for record in self.branch()? {
            if let Some(timestamp) = record["timestamp"].as_str() {
                timestamp.clone_into(&mut updated);
            }
            match record["type"].as_str() {
                Some("message") => {
                    let message = &record["message"];
                    if message["role"] == "user" {
                        active = true;
                    }
                    if message["role"] == "assistant" {
                        active = message["stopReason"] == "toolUse";
                        if let (Some(provider), Some(model)) =
                            (message["provider"].as_str(), message["model"].as_str())
                        {
                            config.model = Some(json!([provider, model]).to_string());
                        }
                    }
                    stream
                        .message(message)
                        .map_err(|_| AgentSessionError::Failed)?;
                    messages.extend(stream.events.drain(..).filter_map(|event| match event {
                        AgentTurnEvent::Timeline(entry) => Some(entry),
                        _ => None,
                    }));
                }
                Some("model_change")
                    if record["role"].as_str().is_none_or(|role| role == "default") =>
                {
                    let route = config::text(record, "model")?
                        .split_once('/')
                        .ok_or(AgentSessionError::Failed)?;
                    config.model = Some(json!([route.0, route.1]).to_string());
                }
                Some("thinking_level_change") => {
                    config.thinking_option_id = record["thinkingLevel"].as_str().map(str::to_owned);
                }
                Some("title_change") => title = record["title"].as_str().map(str::to_owned),
                _ => {}
            }
        }
        let users: Vec<_> = messages
            .iter()
            .filter(|entry| entry.item["type"] == "user_message")
            .collect();
        let preview = |entry: &&crate::protocol::timeline::NativeItem| {
            entry.item["text"]
                .as_str()
                .map(|text| text.chars().take(300).collect::<String>())
        };
        let descriptor = SessionDescriptor {
            provider_id: "omp".to_owned(),
            provider_label: "Oh My Pi".to_owned(),
            provider_handle_id: id.to_owned(),
            cwd: config::text(&self.header, "cwd")?.to_owned(),
            title,
            first_prompt_preview: users.first().and_then(preview),
            last_prompt_preview: users.last().and_then(preview),
            last_activity_at: updated,
        };
        let resume_metadata = self.resume_metadata(handle, &stream, &mut config);
        let created_at = self.header["timestamp"]
            .as_str()
            .ok_or(AgentSessionError::Failed)?
            .to_owned();
        Ok((
            SessionHistory {
                resume_metadata,
                parent_id: None,
                descriptor,
                created_at,
                config,
                active,
                entries: messages,
            },
            stream,
        ))
    }
}

pub(super) async fn inspect(
    client: &OmpClient,
    handle: &AgentPersistenceHandle,
    cwd: &str,
) -> Result<SessionHistory, AgentSessionError> {
    let journal = load(client, handle, cwd).await?;
    let client = client.clone();
    let handle = handle.clone();
    tokio::task::spawn_blocking(move || {
        journal
            .project(&client, &handle)
            .map(|(history, _)| history)
    })
    .await
    .map_err(|_| AgentSessionError::Failed)?
}

pub(super) async fn list(
    client: &OmpClient,
    options: &ListOptions,
) -> Result<Vec<SessionDescriptor>, AgentSessionError> {
    if !(1..=4096).contains(&options.scan_limit) {
        return Err(AgentSessionError::Rejected);
    }
    let root = root(client)?;
    let client = client.clone();
    let options = options.clone();
    tokio::task::spawn_blocking(move || {
        let mut sessions = Vec::new();
        let mut bytes = 0;
        for path in paths(&root)? {
            let size = std::fs::metadata(&path)
                .map_err(|_| AgentSessionError::Failed)?
                .len();
            bytes += size;
            if bytes > 256 * 1024 * 1024 {
                break;
            }
            let Ok(journal) = read(path) else {
                continue;
            };
            if options
                .cwd
                .as_ref()
                .is_some_and(|cwd| journal.header["cwd"] != *cwd)
            {
                continue;
            }
            let handle = AgentPersistenceHandle {
                provider: "omp".to_owned(),
                session_id: config::text(&journal.header, "id")?.to_owned(),
                native_handle: None,
                metadata: None,
            };
            if let Ok((history, _)) = journal.project(&client, &handle)
                && !history.entries.is_empty()
            {
                sessions.push(history.descriptor);
            }
            if sessions.len() >= options.scan_limit {
                break;
            }
        }
        sessions.sort_by(|left, right| right.last_activity_at.cmp(&left.last_activity_at));
        Ok(sessions)
    })
    .await
    .map_err(|_| AgentSessionError::Failed)?
}

fn restore_input_ids(
    stream: &mut Stream,
    handle: &AgentPersistenceHandle,
) -> Result<(), AgentSessionError> {
    if let Some(values) = handle
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("inputIds"))
        .and_then(Value::as_object)
    {
        if values.len() > 4096 {
            return Err(AgentSessionError::Failed);
        }
        stream.input_ids.extend(
            values
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests;
