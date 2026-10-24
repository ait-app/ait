//! Read-only discovery and branch projection for Pi version-three JSONL sessions.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs::{File, Metadata};
use std::io::{BufRead, BufReader, Read, Take};
use std::path::{Component, Path, PathBuf};
use std::time::Instant;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};
use serde_json::{Value, json};
use url::Url;
use uuid::Uuid;

use super::{PROVIDER, PiClient, config, streaming::Stream};
use crate::ports::agent_session::{AgentSessionError, AgentTurnEvent};
use crate::ports::native_history::{ListOptions, SessionDescriptor, SessionHistory};
use crate::protocol::timeline::NativeItem;

const MAX_HISTORY: u64 = 32 * 1024 * 1024;
const MAX_HISTORY_RECORDS: usize = 100_000;
const MAX_HISTORY_ENTRIES: usize = 4096;
const MAX_FILES: usize = 16_384;
const MAX_LIST_BYTES: u64 = 64 * 1024 * 1024;
const MAX_LIST_DURATION: std::time::Duration = std::time::Duration::from_secs(2);
const LIST_CANDIDATE_MULTIPLIER: usize = 8;
const MAX_SETTINGS: u64 = 1024 * 1024;

/// Host-scoped inputs used by both native writers and read-only discovery.
#[derive(Debug, Clone)]
pub(super) struct SessionEnvironment {
    home: Option<OsString>,
    agent_directory: Option<OsString>,
    session_directory: Option<OsString>,
    server_directory: PathBuf,
}

impl Default for SessionEnvironment {
    fn default() -> Self {
        Self::capture(
            |name| std::env::var_os(name),
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        )
    }
}

impl SessionEnvironment {
    fn capture(
        mut variable: impl FnMut(&str) -> Option<OsString>,
        server_directory: PathBuf,
    ) -> Self {
        Self {
            home: variable("HOME").or_else(|| variable("USERPROFILE")),
            agent_directory: variable("PI_CODING_AGENT_DIR").filter(|value| !value.is_empty()),
            session_directory: variable("PI_CODING_AGENT_SESSION_DIR")
                .filter(|value| !value.is_empty()),
            server_directory,
        }
    }
}

#[cfg(test)]
impl SessionEnvironment {
    pub(super) fn isolated(home: &Path, agent_directory: &Path) -> Self {
        Self {
            home: Some(home.as_os_str().to_owned()),
            agent_directory: Some(agent_directory.as_os_str().to_owned()),
            session_directory: None,
            server_directory: home.to_owned(),
        }
    }

    pub(super) fn without_home(server_directory: &Path) -> Self {
        Self::capture(|_| None, server_directory.to_owned())
    }
}

#[derive(Debug)]
enum SessionLayout {
    Default { root: PathBuf, directory: PathBuf },
    Custom { directory: PathBuf },
}

#[derive(Debug)]
enum SessionSetting {
    Missing,
    Default,
    Custom(OsString),
}

impl SessionLayout {
    fn directory(&self) -> &Path {
        match self {
            Self::Default { directory, .. } | Self::Custom { directory } => directory,
        }
    }
}

fn resolve_path(path: PathBuf, cwd: &Path) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

fn agent_directory(
    configured: Option<&OsStr>,
    home: Option<&OsStr>,
) -> Result<PathBuf, AgentSessionError> {
    match configured {
        Some(value) if value == "~" => {
            Ok(PathBuf::from(home.ok_or(AgentSessionError::Unavailable)?))
        }
        Some(value) if value.to_str().is_some_and(tilde_path) => {
            Ok(PathBuf::from(home.ok_or(AgentSessionError::Unavailable)?)
                .join(&value.to_string_lossy()[2..]))
        }
        Some(value)
            if value
                .to_str()
                .is_some_and(|value| value.starts_with("file://")) =>
        {
            Url::parse(value.to_str().ok_or(AgentSessionError::Failed)?)
                .ok()
                .and_then(|url| url.to_file_path().ok())
                .ok_or(AgentSessionError::Failed)
        }
        Some(value) => Ok(PathBuf::from(value)),
        None => Ok(PathBuf::from(home.ok_or(AgentSessionError::Unavailable)?).join(".pi/agent")),
    }
}

fn tilde_path(value: &str) -> bool {
    value.starts_with("~/") || cfg!(windows) && value.starts_with("~\\")
}

fn setting(path: &Path) -> Result<SessionSetting, AgentSessionError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SessionSetting::Missing);
        }
        Err(_) => return Ok(SessionSetting::Missing),
    };
    if file
        .metadata()
        .map_or(true, |metadata| metadata.len() > MAX_SETTINGS)
    {
        return Err(AgentSessionError::Failed);
    }
    let mut bytes = Vec::new();
    file.take(MAX_SETTINGS + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| AgentSessionError::Failed)?;
    if bytes.len() as u64 > MAX_SETTINGS {
        return Err(AgentSessionError::Failed);
    }
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
    let Ok(settings) = serde_json::from_slice::<Value>(bytes) else {
        // Pi records a settings diagnostic and treats a malformed scope as empty.
        return Ok(SessionSetting::Missing);
    };
    let Some(value) = settings
        .as_object()
        .and_then(|settings| settings.get("sessionDir"))
    else {
        return Ok(SessionSetting::Missing);
    };
    match value {
        Value::String(value) if !value.is_empty() => Ok(SessionSetting::Custom(value.into())),
        Value::Null | Value::Bool(false) | Value::String(_) => Ok(SessionSetting::Default),
        Value::Number(value) if value.as_i64() == Some(0) => Ok(SessionSetting::Default),
        _ => Err(AgentSessionError::Failed),
    }
}

fn configured_session_directory(
    client: &PiClient,
    cwd: &Path,
    include_project: bool,
) -> Result<Option<PathBuf>, AgentSessionError> {
    if let Some(directory) = &client.session_environment.session_directory {
        return Ok(Some(resolve_path(
            agent_directory(Some(directory), client.session_environment.home.as_deref())?,
            cwd,
        )));
    }
    let agent_root = resolve_path(
        agent_directory(
            client.session_environment.agent_directory.as_deref(),
            client.session_environment.home.as_deref(),
        )?,
        cwd,
    );
    let global = setting(&agent_root.join("settings.json"))?;
    let configured = if include_project {
        match setting(&cwd.join(".pi/settings.json"))? {
            SessionSetting::Missing => global,
            project => project,
        }
    } else {
        global
    };
    match configured {
        SessionSetting::Custom(directory) => {
            agent_directory(Some(&directory), client.session_environment.home.as_deref())
                .map(|directory| resolve_path(directory, cwd))
                .map(Some)
        }
        SessionSetting::Missing | SessionSetting::Default => Ok(None),
    }
}

fn layout(
    client: &PiClient,
    cwd: &Path,
    include_project: bool,
) -> Result<SessionLayout, AgentSessionError> {
    if let Some(directory) = configured_session_directory(client, cwd, include_project)? {
        return Ok(SessionLayout::Custom { directory });
    }
    let agent_directory = resolve_path(
        agent_directory(
            client.session_environment.agent_directory.as_deref(),
            client.session_environment.home.as_deref(),
        )?,
        cwd,
    );
    let root = agent_directory.join("sessions");
    let directory = project_directory(&root, cwd)?;
    Ok(SessionLayout::Default { root, directory })
}

/// Resolve the exact host-owned directory passed to Pi for this project.
pub(super) fn session_directory(
    client: &PiClient,
    cwd: &Path,
) -> Result<PathBuf, AgentSessionError> {
    Ok(layout(client, cwd, true)?.directory().to_owned())
}

fn project_directory(root: &Path, cwd: &Path) -> Result<PathBuf, AgentSessionError> {
    Ok(root.join(format!("--{}--", project_encoding(cwd)?)))
}

fn project_encoding(cwd: &Path) -> Result<String, AgentSessionError> {
    let cwd = lexical_normalize(cwd);
    let cwd = cwd.to_str().ok_or(AgentSessionError::Rejected)?;
    let cwd = cwd
        .strip_prefix('/')
        .or_else(|| cwd.strip_prefix('\\'))
        .unwrap_or(cwd);
    Ok(cwd.replace(['/', '\\', ':'], "-"))
}

fn lexical_normalize(path: &Path) -> PathBuf {
    path.components()
        .fold(PathBuf::new(), |mut result, component| {
            match component {
                Component::Prefix(prefix) => result.push(prefix.as_os_str()),
                Component::RootDir => result.push(component.as_os_str()),
                Component::CurDir => {}
                Component::ParentDir => {
                    result.pop();
                }
                Component::Normal(segment) => result.push(segment),
            }
            result
        })
}

fn discovered_project_cwd(
    root: &Path,
    file: &Path,
    native_cwd: &str,
    deadline: Instant,
) -> Option<String> {
    let directory = file.parent()?;
    if directory.parent()? != root {
        return None;
    }
    let encoded = directory.file_name()?.to_str()?;
    let encoded = encoded.strip_prefix("--")?.strip_suffix("--")?;
    let canonical = Path::new(native_cwd).canonicalize().ok()?;
    let filesystem_root = canonical.ancestors().last()?;
    let encoded = encoded.strip_prefix(&project_encoding(filesystem_root).ok()?)?;
    let mut remaining_paths = MAX_FILES;
    recover_project_cwd(
        filesystem_root,
        encoded,
        &canonical,
        &mut remaining_paths,
        deadline,
    )
    .and_then(|path| path.into_os_string().into_string().ok())
}

fn recover_project_cwd(
    parent: &Path,
    encoded: &str,
    canonical: &Path,
    remaining_paths: &mut usize,
    deadline: Instant,
) -> Option<PathBuf> {
    if Instant::now() >= deadline {
        return None;
    }
    if encoded.is_empty() {
        return (parent.canonicalize().ok()?.as_path() == canonical).then(|| parent.to_owned());
    }
    let boundaries = encoded
        .char_indices()
        .filter_map(|(index, character)| (character == '-').then_some(index))
        .chain(std::iter::once(encoded.len()));
    for boundary in boundaries {
        if Instant::now() >= deadline {
            return None;
        }
        if boundary == 0 || *remaining_paths == 0 {
            continue;
        }
        *remaining_paths -= 1;
        let candidate = parent.join(&encoded[..boundary]);
        if !candidate.is_dir() {
            continue;
        }
        if boundary == encoded.len() {
            if candidate.canonicalize().ok().as_deref() == Some(canonical) {
                return Some(candidate);
            }
            continue;
        }
        if let Some(path) = recover_project_cwd(
            &candidate,
            &encoded[boundary + 1..],
            canonical,
            remaining_paths,
            deadline,
        ) {
            return Some(path);
        }
    }
    None
}

fn files(
    client: &PiClient,
    project_cwd: Option<&Path>,
    deadline: Option<Instant>,
) -> Result<Vec<PathBuf>, AgentSessionError> {
    let directories = match project_cwd {
        Some(cwd) => match layout(client, cwd, true)? {
            SessionLayout::Default { directory, .. } | SessionLayout::Custom { directory } => {
                vec![directory]
            }
        },
        None => match layout(client, &client.session_environment.server_directory, false)? {
            SessionLayout::Default { root, .. } => project_directories(&root, deadline)?,
            SessionLayout::Custom { directory } => vec![directory],
        },
    };
    files_from_directories(directories, deadline)
}

fn project_directories(
    root: &Path,
    deadline: Option<Instant>,
) -> Result<Vec<PathBuf>, AgentSessionError> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(AgentSessionError::Failed),
    };
    let mut directories = Vec::new();
    for entry in entries.take(MAX_FILES) {
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            break;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(AgentSessionError::Failed),
        };
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(AgentSessionError::Failed),
        };
        if file_type.is_dir() {
            directories.push(entry.path());
        }
    }
    Ok(directories)
}

fn files_from_directories(
    directories: Vec<PathBuf>,
    deadline: Option<Instant>,
) -> Result<Vec<PathBuf>, AgentSessionError> {
    let mut paths = Vec::new();
    for directory in directories {
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            break;
        }
        let metadata = match directory.symlink_metadata() {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(AgentSessionError::Failed),
        };
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            continue;
        }
        let entries = match std::fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(AgentSessionError::Failed),
        };
        for entry in entries.take(MAX_FILES.saturating_sub(paths.len())) {
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                break;
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return Err(AgentSessionError::Failed),
            };
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return Err(AgentSessionError::Failed),
            };
            if entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "jsonl")
                && file_type.is_file()
            {
                let modified = entry
                    .metadata()
                    .and_then(|metadata| metadata.modified())
                    .ok();
                paths.push((modified, entry.path()));
            }
        }
        if paths.len() == MAX_FILES {
            break;
        }
    }
    paths.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
    Ok(paths.into_iter().map(|(_, path)| path).collect())
}

/// Keep the filtered header and remaining records on the same bounded file reader.
struct HistoryReader {
    reader: BufReader<Take<File>>,
    metadata: Metadata,
    header: Value,
    header_bytes: usize,
}

impl HistoryReader {
    fn open(path: &Path) -> Result<Self, AgentSessionError> {
        let mut byte_budget = MAX_HISTORY;
        Self::open_with_budget(path, &mut byte_budget)?.ok_or(AgentSessionError::Failed)
    }

    fn open_with_budget(
        path: &Path,
        byte_budget: &mut u64,
    ) -> Result<Option<Self>, AgentSessionError> {
        let metadata = path
            .symlink_metadata()
            .map_err(|_| AgentSessionError::Failed)?;
        if !metadata.is_file() || metadata.len() > MAX_HISTORY {
            return Err(AgentSessionError::Failed);
        }
        if metadata.len() > *byte_budget {
            return Ok(None);
        }
        let file = File::open(path).map_err(|_| AgentSessionError::Failed)?;
        let metadata = file.metadata().map_err(|_| AgentSessionError::Failed)?;
        if !metadata.is_file() || metadata.len() > MAX_HISTORY {
            return Err(AgentSessionError::Failed);
        }
        if metadata.len() > *byte_budget {
            return Ok(None);
        }
        *byte_budget -= metadata.len();
        let mut reader = BufReader::new(file.take(metadata.len()));
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|_| AgentSessionError::Failed)?;
        if !line.ends_with('\n') || line.len() as u64 > MAX_HISTORY {
            return Err(AgentSessionError::Failed);
        }
        let header: Value = serde_json::from_str(&line).map_err(|_| AgentSessionError::Failed)?;
        if header["type"] != "session"
            || header["version"] != 3
            || header["id"]
                .as_str()
                .is_none_or(|id| Uuid::parse_str(id).is_err())
            || header["cwd"]
                .as_str()
                .is_none_or(|cwd| !Path::new(cwd).is_absolute())
        {
            return Err(AgentSessionError::Rejected);
        }
        Ok(Some(Self {
            reader,
            metadata,
            header,
            header_bytes: line.len(),
        }))
    }

    fn records(mut self) -> Result<Vec<Value>, AgentSessionError> {
        let mut data = String::new();
        self.reader
            .read_to_string(&mut data)
            .map_err(|_| AgentSessionError::Failed)?;
        let after = self
            .reader
            .get_ref()
            .get_ref()
            .metadata()
            .map_err(|_| AgentSessionError::Failed)?;
        if (self.header_bytes + data.len()) as u64 > MAX_HISTORY
            || (!data.is_empty() && !data.ends_with('\n'))
            || self.metadata.len() != after.len()
            || self.metadata.modified().ok() != after.modified().ok()
            || data.lines().nth(MAX_HISTORY_RECORDS).is_some()
        {
            return Err(AgentSessionError::Failed);
        }
        let mut records = vec![self.header];
        for line in data.lines() {
            records.push(serde_json::from_str(line).map_err(|_| AgentSessionError::Failed)?);
        }
        for record in &records {
            chrono::DateTime::parse_from_rfc3339(field(record, "timestamp")?)
                .map_err(|_| AgentSessionError::Failed)?;
        }
        Ok(records)
    }
}

fn field<'a>(value: &'a Value, key: &str) -> Result<&'a str, AgentSessionError> {
    value[key]
        .as_str()
        .filter(|text| !text.is_empty())
        .ok_or(AgentSessionError::Failed)
}

fn branch(records: &[Value]) -> Result<Vec<&Value>, AgentSessionError> {
    let mut index = BTreeMap::new();
    for record in &records[1..] {
        let id = field(record, "id")?;
        if id.len() > 512 {
            return Err(AgentSessionError::Failed);
        }
        if index.insert(id, record).is_some() {
            return Err(AgentSessionError::Failed);
        }
    }
    let mut path = Vec::new();
    let mut seen = BTreeSet::new();
    let mut current = records.get(1..).and_then(|entries| entries.last());
    while let Some(record) = current {
        if !seen.insert(field(record, "id")?) {
            return Err(AgentSessionError::Failed);
        }
        path.push(record);
        current = match record.get("parentId") {
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

fn preview(message: &Value) -> Option<String> {
    let text = if let Some(text) = message["content"].as_str() {
        text.to_owned()
    } else {
        message["content"]
            .as_array()?
            .iter()
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n")
    };
    let text = text.trim();
    (!text.is_empty()).then(|| text.chars().take(500).collect())
}

fn descriptor(records: &[Value], path: &[&Value]) -> Result<SessionDescriptor, AgentSessionError> {
    let header = &records[0];
    let prompts: Vec<_> = path
        .iter()
        .filter(|entry| entry["type"] == "message" && entry["message"]["role"] == "user")
        .filter_map(|entry| preview(&entry["message"]))
        .collect();
    Ok(SessionDescriptor {
        provider_id: PROVIDER.into(),
        provider_label: "Pi".into(),
        provider_handle_id: field(header, "id")?.into(),
        cwd: field(header, "cwd")?.into(),
        title: path
            .iter()
            .rev()
            .find(|entry| entry["type"] == "session_info")
            .and_then(|entry| entry["name"].as_str())
            .map(|name| name.chars().take(500).collect()),
        first_prompt_preview: prompts.first().cloned(),
        last_prompt_preview: prompts.last().cloned(),
        last_activity_at: field(
            records.last().ok_or(AgentSessionError::Failed)?,
            "timestamp",
        )?
        .into(),
    })
}

/// List bounded native candidates without starting Pi or modifying its session files.
pub(super) fn list(
    client: &PiClient,
    options: &ListOptions,
) -> Result<Vec<SessionDescriptor>, AgentSessionError> {
    list_before(client, options, Instant::now() + MAX_LIST_DURATION)
}

fn list_before(
    client: &PiClient,
    options: &ListOptions,
    deadline: Instant,
) -> Result<Vec<SessionDescriptor>, AgentSessionError> {
    if !(1..=4096).contains(&options.scan_limit) {
        return Err(AgentSessionError::Rejected);
    }
    let mut sessions = Vec::new();
    let mut seen = BTreeSet::new();
    // Pi encodes the supplied cwd spelling into its default session directory. Keep that
    // spelling for lookup, while using the canonical path only to compare header identity.
    let cwd = options.cwd.as_deref().map(Path::new);
    let Ok(canonical_cwd) = cwd.map(std::fs::canonicalize).transpose() else {
        return Ok(Vec::new());
    };
    let default_root = if cwd.is_none() {
        match layout(client, &client.session_environment.server_directory, false)? {
            SessionLayout::Default { root, .. } => Some(root),
            SessionLayout::Custom { .. } => None,
        }
    } else {
        None
    };
    let candidate_limit = options
        .scan_limit
        .saturating_mul(LIST_CANDIDATE_MULTIPLIER)
        .min(MAX_FILES);
    let mut byte_budget = MAX_LIST_BYTES;
    let mut project_cwds = BTreeMap::new();
    for path in files(client, cwd, Some(deadline))?
        .into_iter()
        .take(candidate_limit)
    {
        if Instant::now() >= deadline {
            break;
        }
        let opened = match HistoryReader::open_with_budget(&path, &mut byte_budget) {
            Ok(Some(opened)) => Ok(opened),
            Ok(None) => break,
            Err(error) => Err(error),
        };
        if let Some(cwd) = &canonical_cwd
            && !opened.as_ref().is_ok_and(|history| {
                history.header["cwd"]
                    .as_str()
                    .and_then(|native| std::fs::canonicalize(native).ok())
                    .as_ref()
                    == Some(cwd)
            })
        {
            continue;
        }
        let Ok(records) = opened.and_then(HistoryReader::records) else {
            continue;
        };
        let Ok(current_branch) = branch(&records) else {
            continue;
        };
        // Pi appends model and thinking metadata when a session opens. Only branches
        // with messages have conversation history to import, even if they have no text preview.
        if !current_branch
            .iter()
            .any(|entry| entry["type"] == "message")
        {
            continue;
        }
        let Ok(mut candidate) = descriptor(&records, &current_branch) else {
            continue;
        };
        if let Some(cwd) = cwd {
            candidate.cwd = cwd.to_string_lossy().into_owned();
        } else if let (Some(root), Some(directory)) = (default_root.as_deref(), path.parent()) {
            let recovered = project_cwds
                .entry((directory.to_owned(), candidate.cwd.clone()))
                .or_insert_with(|| discovered_project_cwd(root, &path, &candidate.cwd, deadline));
            if let Some(recovered) = recovered {
                candidate.cwd.clone_from(recovered);
            }
        }
        if !seen.insert(candidate.provider_handle_id.clone()) {
            continue;
        }
        sessions.push(candidate);
        // Invalid files and duplicate identities do not consume the result budget, but do
        // consume the independent candidate, byte, and elapsed-time work budgets above.
        if sessions.len() >= options.scan_limit {
            break;
        }
    }
    Ok(sessions)
}

fn same_directory(left: &str, right: &str) -> bool {
    Path::new(left)
        .canonicalize()
        .ok()
        .zip(Path::new(right).canonicalize().ok())
        .is_some_and(|(left, right)| left == right)
}

fn saved_session_file(
    client: &PiClient,
    handle: &AgentPersistenceHandle,
    cwd: &Path,
) -> Option<PathBuf> {
    let native_path = handle.native_handle.as_ref().and_then(Value::as_str);
    let path = PathBuf::from(
        native_path.or_else(|| handle.metadata.as_ref()?.get("sessionFile")?.as_str())?,
    );
    let parent = path.parent()?;
    let metadata = path.symlink_metadata().ok()?;
    let parent_metadata = parent.symlink_metadata().ok()?;
    if !path.is_absolute()
        || path.extension() != Some(OsStr::new("jsonl"))
        || !metadata.is_file()
        || metadata.file_type().is_symlink()
        || !parent_metadata.is_dir()
        || parent_metadata.file_type().is_symlink()
    {
        return None;
    }
    if native_path.is_some() {
        return Some(path);
    }
    let layout = layout(client, cwd, true).ok()?;
    let candidate_directory = parent.canonicalize().ok()?;
    let contained = match layout {
        SessionLayout::Default { root, .. } => {
            parent.parent()?.canonicalize().ok()? == root.canonicalize().ok()?
        }
        SessionLayout::Custom { directory } => {
            candidate_directory == directory.canonicalize().ok()?
        }
    };
    contained.then_some(path)
}

fn inspect_file(
    client: &PiClient,
    path: &Path,
    history: HistoryReader,
    cwd: &str,
) -> Result<SessionHistory, AgentSessionError> {
    let records = history.records()?;
    let branch = branch(&records)?;
    let mut descriptor = descriptor(&records, &branch)?;
    if !same_directory(cwd, &descriptor.cwd) {
        return Err(AgentSessionError::Rejected);
    }
    cwd.clone_into(&mut descriptor.cwd);
    project(client, path, &records, &branch, descriptor)
}

fn context_branch<'a>(branch: &[&'a Value]) -> Result<Vec<&'a Value>, AgentSessionError> {
    let Some((compaction_index, compaction)) = branch
        .iter()
        .enumerate()
        .rev()
        .find(|(_, record)| record["type"] == "compaction")
    else {
        return Ok(branch.to_vec());
    };
    let first_kept_id = field(compaction, "firstKeptEntryId")?;
    let mut context = Vec::with_capacity(branch.len() - compaction_index);
    context.push(*compaction);
    let mut found_first_kept = false;
    for record in &branch[..compaction_index] {
        if record["id"] == first_kept_id {
            found_first_kept = true;
        }
        let system_message = record["type"] == "message" && record["message"]["role"] == "system";
        let provenance_compaction = record["type"] == "compaction";
        if found_first_kept && !system_message && !provenance_compaction {
            context.push(*record);
        }
    }
    context.extend_from_slice(&branch[compaction_index + 1..]);
    Ok(context)
}

#[derive(Debug, Clone, Copy)]
enum ContentEdit<'a> {
    Omit,
    Replace(&'a Value),
}

fn content_edits<'a>(
    branch: &[&'a Value],
) -> Result<BTreeMap<&'a str, ContentEdit<'a>>, AgentSessionError> {
    let mut edits = BTreeMap::new();
    for record in branch
        .iter()
        .filter(|record| record["type"] == "context_edit")
    {
        let edit = match record.get("replacement") {
            Some(Value::Null) => ContentEdit::Omit,
            Some(Value::Object(replacement)) => {
                let content = replacement
                    .get("content")
                    .filter(|content| content.is_string() || content.is_array())
                    .ok_or(AgentSessionError::Failed)?;
                ContentEdit::Replace(content)
            }
            _ => return Err(AgentSessionError::Failed),
        };
        edits.insert(field(record, "targetId")?, edit);
    }
    Ok(edits)
}

fn project_config(
    branch: &[&Value],
    config: &mut StoredAgentConfig,
) -> Result<(), AgentSessionError> {
    for record in branch {
        if record["type"] == "model_change" {
            config.model = Some(format!(
                "{}/{}",
                field(record, "provider")?,
                field(record, "modelId")?
            ));
        }
        if record["type"] == "message"
            && record["message"]["role"] == "assistant"
            && let (Some(provider), Some(model)) = (
                record["message"]["provider"].as_str(),
                record["message"]["model"].as_str(),
            )
        {
            config.model = Some(format!("{provider}/{model}"));
        }
    }
    Ok(())
}

/// Inspect a verified identity and project its current branch; malformed history is rejected.
pub(super) fn inspect(
    client: &PiClient,
    handle: &AgentPersistenceHandle,
    cwd: &str,
) -> Result<SessionHistory, AgentSessionError> {
    if handle.provider != PROVIDER || Uuid::parse_str(&handle.session_id).is_err() {
        return Err(AgentSessionError::Rejected);
    }
    let lookup_cwd = handle
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("nativeCwd"))
        .and_then(Value::as_str)
        .unwrap_or(cwd);
    if !same_directory(cwd, lookup_cwd) {
        return Err(AgentSessionError::Rejected);
    }
    let cwd = lookup_cwd;
    if let Some(path) = saved_session_file(client, handle, Path::new(cwd)) {
        let history = HistoryReader::open(&path)?;
        if history.header["id"].as_str() != Some(&handle.session_id) {
            return Err(AgentSessionError::Rejected);
        }
        return inspect_file(client, &path, history, cwd);
    }
    // Validate the cwd canonically, but retain its spelling for Pi's directory encoding.
    let cwd_path = Path::new(cwd);
    cwd_path
        .canonicalize()
        .map_err(|_| AgentSessionError::Rejected)?;
    let mut candidate = None;
    for path in files(client, Some(cwd_path), None)? {
        let Ok(history) = HistoryReader::open(&path) else {
            continue;
        };
        if history.header["id"].as_str() != Some(&handle.session_id) {
            continue;
        }
        if candidate.replace((path, history)).is_some() {
            return Err(AgentSessionError::Rejected);
        }
    }
    let (path, history) = candidate.ok_or(AgentSessionError::Unavailable)?;
    inspect_file(client, &path, history, cwd)
}

fn project(
    client: &PiClient,
    file: &Path,
    records: &[Value],
    branch: &[&Value],
    descriptor: SessionDescriptor,
) -> Result<SessionHistory, AgentSessionError> {
    let resume_cwd = Path::new(&descriptor.cwd)
        .canonicalize()
        .map_err(|_| AgentSessionError::Rejected)?
        .into_os_string()
        .into_string()
        .map_err(|_| AgentSessionError::Rejected)?;
    let mut history = SessionHistory {
        resume_metadata: BTreeMap::from([
            ("cwd".into(), json!(resume_cwd)),
            ("nativeCwd".into(), json!(descriptor.cwd)),
            ("hasInput".into(), json!(true)),
            (
                "sessionFile".into(),
                json!(file.canonicalize().map_err(|_| AgentSessionError::Failed)?),
            ),
        ]),
        parent_id: None,
        created_at: field(&records[0], "timestamp")?.into(),
        descriptor,
        config: StoredAgentConfig::default(),
        active: false,
        entries: Vec::new(),
    };
    project_config(branch, &mut history.config)?;
    let context = context_branch(branch)?;
    let edits = content_edits(&context)?;
    let mut stream = Stream::default();
    let mut pending = BTreeMap::new();
    for record in context {
        if record["type"] == "compaction" {
            let id = format!("native:pi:compaction:{}", field(record, "id")?);
            append_entry(
                &mut history,
                NativeItem {
                    key: id.clone(),
                    turn_id: None,
                    timestamp: field(record, "timestamp")?.into(),
                    item: json!({"type":"compaction","compactionId":id,"status":"completed"}),
                },
            )?;
            append_text(&mut history, record, field(record, "summary")?)?;
        }
        if record["type"] == "custom_message" && record["display"] == true {
            match edits.get(field(record, "id")?) {
                Some(ContentEdit::Omit) => {}
                Some(ContentEdit::Replace(replacement)) => {
                    let text = content(client, replacement)?;
                    append_text(&mut history, record, &text)?;
                }
                None => {
                    let text = content(client, &record["content"])?;
                    append_text(&mut history, record, &text)?;
                }
            }
        }
        if record["type"] == "branch_summary" {
            append_text(&mut history, record, field(record, "summary")?)?;
        }
        if record["type"] != "message" {
            continue;
        }
        let images = project_message(
            client,
            &mut stream,
            &mut pending,
            record,
            edits.get(field(record, "id")?).copied(),
            &mut history,
        )?;
        while let Some(event) = stream.pop() {
            if let AgentTurnEvent::Timeline(mut entry) = event {
                entry.timestamp = field(record, "timestamp")?.into();
                append_entry(&mut history, entry)?;
            }
        }
        append_text(&mut history, record, &images.join("\n"))?;
    }
    config::validate(&history.config)?;
    Ok(history)
}

fn project_message(
    client: &PiClient,
    stream: &mut Stream,
    pending: &mut BTreeMap<String, usize>,
    record: &Value,
    edit: Option<ContentEdit<'_>>,
    history: &mut SessionHistory,
) -> Result<Vec<String>, AgentSessionError> {
    let original_message = &record["message"];
    let role = field(original_message, "role")?;
    let editable = matches!(role, "user" | "assistant" | "toolResult" | "custom");
    let omitted = editable && matches!(edit, Some(ContentEdit::Omit));
    let edited_message = match (editable, edit) {
        (true, Some(ContentEdit::Replace(content))) => {
            let mut message = original_message.clone();
            message["content"] = if matches!(role, "assistant" | "toolResult")
                && let Some(text) = content.as_str()
            {
                json!([{"type":"text","text":text}])
            } else {
                content.clone()
            };
            Some(message)
        }
        _ => None,
    };
    let message = (!omitted)
        .then(|| edited_message.as_ref().unwrap_or(original_message))
        .map(|message| normalize_message_content(message, role));
    let message = message.as_deref();
    let id = field(record, "id")?;
    match role {
        // Native system prompts remain provider configuration, not conversation rows.
        "system" => {}
        "custom" => {
            if let Some(message) = message
                && message["display"] == true
            {
                let text = content(client, &message["content"])?;
                append_text(history, record, &text)?;
            }
        }
        "user" => {
            let Some(message) = message else {
                return Ok(Vec::new());
            };
            if !pending.is_empty() {
                return Err(AgentSessionError::Rejected);
            }
            stream.begin(id.into());
            let (text, sources) = user_content(client, &message["content"])?;
            stream
                .events
                .extend(Stream::user_entries(id, None, &text, &sources));
            history.active = true;
        }
        "assistant" => {
            if let Some(message) = message {
                project_assistant_message(
                    stream,
                    pending,
                    message,
                    field(record, "timestamp")?,
                    history,
                )?;
            }
            history.active = !pending.is_empty();
        }
        "toolResult" => {
            let tool_id = field(original_message, "toolCallId")?;
            let tool_index = pending.remove(tool_id).ok_or(AgentSessionError::Failed)?;
            if let Some(message) = message {
                let (result, images) = client.images.split(message)?;
                stream.consume(&json!({"type":"tool_execution_end", "toolCallId":tool_id, "toolName":result["toolName"], "result":result, "isError":result["isError"]}))?;
                let Some(AgentTurnEvent::Timeline(mut entry)) = stream.pop() else {
                    return Err(AgentSessionError::Failed);
                };
                entry
                    .timestamp
                    .clone_from(&history.entries[tool_index].timestamp);
                history.entries[tool_index] = entry;
                // A tool result still awaits the assistant's continuation.
                history.active = true;
                return Ok(images);
            }
            // A tool result still awaits the assistant's continuation.
            history.active = true;
        }
        "bashExecution" => {
            let message = message.ok_or(AgentSessionError::Failed)?;
            stream.consume(&json!({"type":"tool_execution_start", "toolCallId":id,
                    "toolName":"bash", "args":{"command":field(message, "command")?}}))?;
            stream.consume(&json!({"type":"tool_execution_end", "toolCallId":id,
                    "toolName":"bash", "isError":message["cancelled"] == true || message["exitCode"].as_i64().is_some_and(|code| code != 0),
                    "result":{"content":[{"type":"text","text":message["output"].as_str().unwrap_or_default()}]}}))?;
        }
        _ => return Err(AgentSessionError::Rejected),
    }
    Ok(Vec::new())
}

fn project_assistant_message(
    stream: &mut Stream,
    pending: &mut BTreeMap<String, usize>,
    message: &Value,
    timestamp: &str,
    history: &mut SessionHistory,
) -> Result<(), AgentSessionError> {
    let mut entries = stream.historical_message(message)?.into_iter().peekable();
    let blocks = message["content"]
        .as_array()
        .ok_or(AgentSessionError::Failed)?;
    for (block_index, block) in blocks.iter().enumerate() {
        while entries
            .peek()
            .is_some_and(|(entry_block_index, _)| *entry_block_index == block_index)
        {
            let (_, mut entry) = entries.next().ok_or(AgentSessionError::Failed)?;
            entry.timestamp = timestamp.into();
            append_entry(history, entry)?;
        }
        if block["type"] != "toolCall" {
            continue;
        }
        let tool_id = field(block, "id")?;
        if pending.contains_key(tool_id) {
            return Err(AgentSessionError::Failed);
        }
        stream.consume(&json!({"type":"tool_execution_start", "toolCallId":tool_id,
            "toolName":field(block, "name")?, "args":block["arguments"]}))?;
        let Some(AgentTurnEvent::Progress { mut entry, .. }) = stream.pop() else {
            return Err(AgentSessionError::Failed);
        };
        entry.timestamp = timestamp.into();
        let tool_index = history.entries.len();
        append_entry(history, entry)?;
        pending.insert(tool_id.to_owned(), tool_index);
    }
    Ok(())
}

fn normalize_message_content<'a>(message: &'a Value, role: &str) -> Cow<'a, Value> {
    if !message["content"].is_null() {
        return Cow::Borrowed(message);
    }
    let fallback = match role {
        "user" => json!(""),
        "assistant" | "toolResult" => json!([]),
        _ => return Cow::Borrowed(message),
    };
    let mut normalized = message.clone();
    normalized["content"] = fallback;
    Cow::Owned(normalized)
}

fn append_text(
    history: &mut SessionHistory,
    record: &Value,
    text: &str,
) -> Result<(), AgentSessionError> {
    let mut remaining = text;
    let mut index = 0;
    while !remaining.is_empty() {
        let end = remaining.floor_char_boundary(remaining.len().min(96 * 1024));
        let key = format!("native:pi:entry:{}:{index}", field(record, "id")?);
        append_entry(
            history,
            NativeItem {
                key: key.clone(),
                turn_id: None,
                timestamp: field(record, "timestamp")?.into(),
                item: json!({"type":"assistant_message","messageId":key,"text":&remaining[..end]}),
            },
        )?;
        remaining = &remaining[end..];
        index += 1;
    }
    Ok(())
}

fn append_entry(history: &mut SessionHistory, entry: NativeItem) -> Result<(), AgentSessionError> {
    if history.entries.len() >= MAX_HISTORY_ENTRIES {
        return Err(AgentSessionError::Failed);
    }
    history.entries.push(entry);
    Ok(())
}

fn user_content(
    client: &PiClient,
    value: &Value,
) -> Result<(String, Vec<String>), AgentSessionError> {
    if let Some(text) = value.as_str() {
        return Ok((text.into(), Vec::new()));
    }
    let blocks = value.as_array().ok_or(AgentSessionError::Failed)?;
    let mut text = Vec::new();
    let mut sources = Vec::new();
    for block in blocks {
        match field(block, "type")? {
            "text" => text.push(block["text"].as_str().ok_or(AgentSessionError::Failed)?),
            "image" => sources.push(client.images.source(block)?.into_owned()),
            _ => return Err(AgentSessionError::Rejected),
        }
    }
    Ok((text.join("\n"), sources))
}

fn content(client: &PiClient, value: &Value) -> Result<String, AgentSessionError> {
    if let Some(text) = value.as_str() {
        return Ok(text.into());
    }
    value
        .as_array()
        .ok_or(AgentSessionError::Failed)?
        .iter()
        .map(|block| match field(block, "type")? {
            "text" => Ok(block["text"]
                .as_str()
                .ok_or(AgentSessionError::Failed)?
                .to_owned()),
            "image" => client.images.render(block),
            _ => Err(AgentSessionError::Rejected),
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|parts| parts.join("\n"))
}

#[cfg(test)]
mod tests;
