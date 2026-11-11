//! OMP subagent transcripts in history (M6, D16).
//!
//! OMP 18.18.8 records a child's session at `<parent file without .jsonl>/<job>.jsonl`; the
//! parent's `task` result names the job (`details.results[].id` for a finished task, or
//! `details.progress[].id` and `details.async.jobId` for a background one), and the parent's
//! `async-result` message reports a background child's outcome. As in Paseo, a transcript is
//! linked only from the parent's active branch, and nested children are read recursively.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use domain::agent_runtime::AgentPersistenceHandle;
use serde_json::{Value, json};

use super::super::{OmpClient, PROVIDER, config, subagents};
use super::{Journal, Workspace, paths, reader};
use provider::ports::agent_session::AgentSessionError;
use provider::ports::controls::NativeSubagent;
use provider::ports::native_history::SessionHistory;

/// Metadata key of the locator on a child's persistence handle.
const LOCATOR: &str = "ompSubagent";
/// Nesting levels below a root session that are read.
const MAX_DEPTH: usize = 4;
/// Children discovered per workspace.
const MAX_CHILDREN: usize = 4096;
/// Bytes of a job id, which names a file.
const MAX_JOB_BYTES: usize = 128;

/// The native identity of child `job` of session `parent`. OMP job ids (`Prober`) are unique
/// only within their parent, so the parent's identity qualifies them; a grandchild's parent
/// is the child's qualified identity.
pub(crate) fn child_id(parent: &str, job: &str) -> String {
    format!("{parent}/{job}")
}

/// Whether `job` can name a transcript file: ASCII letters, digits, `.`, `_` and `-`, and
/// neither `.` nor `..`.
pub(crate) fn valid_job(job: &str) -> bool {
    !job.is_empty()
        && job.len() <= MAX_JOB_BYTES
        && job != "."
        && job != ".."
        && job
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// The transcript of child `job` of the session file `parent`.
fn transcript(parent: &Path, job: &str) -> PathBuf {
    parent.with_extension("").join(format!("{job}.jsonl"))
}

/// The persistence handle that locates child `job` of the session `parent` recorded in
/// `parent_file`, for `inspect_session`.
pub(crate) fn handle(parent_file: &Path, parent: &str, job: &str) -> AgentPersistenceHandle {
    AgentPersistenceHandle {
        provider: PROVIDER.to_owned(),
        session_id: child_id(parent, job),
        native_handle: None,
        metadata: Some(BTreeMap::from([(
            LOCATOR.to_owned(),
            json!({"parentFile":parent_file,"parentId":parent,"job":job}),
        )])),
    }
}

/// What a parent records about one child job.
#[derive(Debug, Default, Clone, PartialEq)]
struct Link {
    /// The `task` call that started the child.
    call: String,
    agent: Option<String>,
    description: Option<String>,
    /// A finished child's outcome, when the parent recorded it.
    status: Option<&'static str>,
}

/// The children that `entries`, a parent's active branch, link to, by job id.
fn links(entries: &[Value]) -> BTreeMap<String, Link> {
    let mut calls = BTreeMap::new();
    let mut links = BTreeMap::new();
    for entry in entries {
        let message = &entry["message"];
        match (entry["type"].as_str(), message["role"].as_str()) {
            (Some("message"), Some("assistant")) => {
                for block in message["content"].as_array().into_iter().flatten() {
                    if block["type"] == "toolCall"
                        && block["name"] == "task"
                        && let Some(id) = block["id"].as_str()
                    {
                        calls.insert(id, text(&block["arguments"]["agent"]));
                    }
                }
            }
            (Some("message"), Some("toolResult")) if message["toolName"] == "task" => {
                let Some(call) = message["toolCallId"].as_str() else {
                    continue;
                };
                let Some(agent) = calls.get(call) else {
                    continue;
                };
                task_links(message, call, *agent, &mut links);
            }
            (Some("custom_message"), _) if entry["customType"] == "async-result" => {
                job_outcomes(&entry["details"]["jobs"], &mut links);
            }
            _ => {}
        }
    }
    links
}

/// Add the jobs one `task` result names: its finished `results`, else its background
/// `progress` and `async.jobId`.
fn task_links(
    message: &Value,
    call: &str,
    agent: Option<&str>,
    links: &mut BTreeMap<String, Link>,
) {
    let details = &message["details"];
    let failed = message["isError"] == true;
    let results = details["results"]
        .as_array()
        .filter(|results| !results.is_empty());
    let jobs = results.or_else(|| details["progress"].as_array());
    for job in jobs.into_iter().flatten() {
        let Some(id) = job["id"].as_str().filter(|id| valid_job(id)) else {
            continue;
        };
        links.insert(
            id.to_owned(),
            Link {
                call: call.to_owned(),
                agent: text(&job["agent"]).or(agent).map(str::to_owned),
                description: text(&job["description"])
                    .or_else(|| text(&job["assignment"]))
                    .map(str::to_owned),
                status: results.map(|_| result_status(job, failed)),
            },
        );
    }
    if let Some(job) = text(&details["async"]["jobId"]).filter(|job| valid_job(job)) {
        links.entry(job.to_owned()).or_insert_with(|| Link {
            call: call.to_owned(),
            agent: agent.map(str::to_owned),
            ..Link::default()
        });
    }
}

/// Paseo's `taskResultStatus`.
fn result_status(result: &Value, failed: bool) -> &'static str {
    let error = !result["error"].is_null() && result["error"] != false && result["error"] != "";
    if failed || error || result["exitCode"].as_i64().is_some_and(|code| code != 0) {
        "failed"
    } else if result["aborted"] == true {
        "canceled"
    } else {
        "completed"
    }
}

/// Record the outcomes an `async-result` message reports for background task jobs.
fn job_outcomes(jobs: &Value, links: &mut BTreeMap<String, Link>) {
    for job in jobs.as_array().into_iter().flatten() {
        if job["type"] != "task" {
            continue;
        }
        let Some(link) = text(&job["agentId"])
            .or_else(|| text(&job["jobId"]))
            .and_then(|id| links.get_mut(id))
        else {
            continue;
        };
        link.status = match job["status"].as_str() {
            Some("completed") => Some("completed"),
            Some("failed") => Some("failed"),
            Some("aborted" | "cancelled" | "canceled") => Some("canceled"),
            _ => link.status,
        };
    }
}

fn text(value: &Value) -> Option<&str> {
    value
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

/// The model a child last used: its last `model_change` (`provider/id`) or assistant message.
fn model(entries: &[Value]) -> Option<String> {
    entries
        .iter()
        .rev()
        .find_map(|entry| match entry["type"].as_str() {
            Some("model_change") => text(&entry["model"]).map(str::to_owned),
            Some("message") if entry["message"]["role"] == "assistant" => {
                let message = &entry["message"];
                let provider = text(&message["provider"])?;
                let model = text(&message["responseModel"]).or_else(|| text(&message["model"]))?;
                Some(format!("{provider}/{model}"))
            }
            _ => None,
        })
}

/// Whether a branch ends waiting for the model: after a user message or a tool call.
fn active(entries: &[Value]) -> bool {
    entries
        .iter()
        .rev()
        .find_map(|entry| {
            let message = &entry["message"];
            (entry["type"] == "message").then(|| match message["role"].as_str() {
                Some("user") => Some(true),
                Some("assistant") => Some(message["stopReason"] == "toolUse"),
                _ => None,
            })?
        })
        .unwrap_or(false)
}

/// Every child linked from the sessions recorded for workspace `cwd`, without starting OMP.
/// A session that cannot be read is skipped.
///
/// # Errors
///
/// Returns failed when the sessions directory cannot be listed, unavailable without a home
/// directory, or rejected for an invalid profile.
pub(super) fn list(
    client: &OmpClient,
    cwd: &str,
) -> Result<Vec<NativeSubagent>, AgentSessionError> {
    let root = paths::sessions(client, Some(cwd))?;
    let workspace = Workspace::new(cwd);
    let mut children = Vec::new();
    for path in paths::scan(&root)? {
        // Only a session with a directory of artifacts can have children.
        if !path.with_extension("").is_dir() {
            continue;
        }
        let Ok(parent) = reader::read(&path) else {
            continue;
        };
        let Ok(id) = config::text(&parent.header, "id") else {
            continue;
        };
        if !workspace.matches(&parent.header["cwd"]) {
            continue;
        }
        let parent = Parent {
            file: &path,
            id,
            entries: &parent.entries,
        };
        collect(&parent, 0, &mut children);
        if children.len() >= MAX_CHILDREN {
            break;
        }
    }
    Ok(children)
}

/// A session whose children are being collected.
struct Parent<'a> {
    file: &'a Path,
    id: &'a str,
    entries: &'a [Value],
}

fn collect(parent: &Parent<'_>, depth: usize, children: &mut Vec<NativeSubagent>) {
    for (job, link) in links(parent.entries) {
        if children.len() >= MAX_CHILDREN {
            return;
        }
        let file = transcript(parent.file, &job);
        let Ok(child) = reader::read(&file) else {
            continue;
        };
        let id = child_id(parent.id, &job);
        children.push(describe(parent, &job, &link, &child));
        if depth + 1 < MAX_DEPTH && file.with_extension("").is_dir() {
            let nested = Parent {
                file: &file,
                id: &id,
                entries: &child.entries,
            };
            collect(&nested, depth + 1, children);
        }
    }
}

/// The child descriptor Paseo's replay publishes: title "agent · model (provider)", the
/// task's description, and the recorded outcome, else running while the transcript waits.
fn describe(
    parent: &Parent<'_>,
    job: &str,
    link: &Link,
    child: &reader::Snapshot,
) -> NativeSubagent {
    let id = child_id(parent.id, job);
    let cwd = child.header["cwd"].as_str().unwrap_or_default().to_owned();
    let created = child.header["timestamp"].clone();
    let updated = child
        .entries
        .iter()
        .rev()
        .find_map(|entry| entry["timestamp"].as_str())
        .map_or_else(|| created.clone(), |time| json!(time));
    let status = link.status.unwrap_or(if active(&child.entries) {
        "running"
    } else {
        "completed"
    });
    let title = subagents::title(link.agent.as_deref(), model(&child.entries).as_deref());
    NativeSubagent {
        persistence: Some(handle(parent.file, parent.id, job)),
        id: id.clone(),
        parent_id: parent.id.to_owned(),
        cwd: cwd.clone(),
        descriptor: json!({"id":id,"provider":PROVIDER,"title":title,"cwd":cwd,
            "description":link.description,"createdAt":created,"updatedAt":updated,
            "toolCallId":link.call,"status":status}),
    }
}

/// The locator a child handle carries, validated before any file is read.
struct Locator {
    parent_file: PathBuf,
    parent: String,
    job: String,
}

impl Locator {
    fn new(handle: &AgentPersistenceHandle) -> Result<Self, AgentSessionError> {
        let locator = handle
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get(LOCATOR))
            .ok_or(AgentSessionError::Rejected)?;
        let parent_file = locator["parentFile"]
            .as_str()
            .map(PathBuf::from)
            .filter(|path| path.is_absolute() && path.extension().is_some_and(|ext| ext == "jsonl"))
            .ok_or(AgentSessionError::Rejected)?;
        let parent = config::text(locator, "parentId").map_err(|_| AgentSessionError::Rejected)?;
        let job = locator["job"]
            .as_str()
            .filter(|job| valid_job(job))
            .ok_or(AgentSessionError::Rejected)?;
        if handle.provider != PROVIDER || handle.session_id != child_id(parent, job) {
            return Err(AgentSessionError::Rejected);
        }
        Ok(Self {
            parent_file,
            parent: parent.to_owned(),
            job: job.to_owned(),
        })
    }
}

/// Whether `handle` locates a subagent transcript rather than a root session.
pub(super) fn is_child(handle: &AgentPersistenceHandle) -> bool {
    handle
        .metadata
        .as_ref()
        .is_some_and(|metadata| metadata.contains_key(LOCATOR))
}

/// Read and project the child transcript `handle` locates, for workspace `cwd`. Its history
/// names the parent session as its immediate parent; this blocks on file-system I/O.
///
/// # Errors
///
/// Returns rejected for an invalid locator, a transcript of another workspace or one OMP
/// recorded for another parent; unavailable for a missing transcript; failed when unreadable.
pub(super) fn read(
    client: &OmpClient,
    handle: &AgentPersistenceHandle,
    cwd: &str,
) -> Result<SessionHistory, AgentSessionError> {
    let locator = Locator::new(handle)?;
    let path = transcript(&locator.parent_file, &locator.job);
    let snapshot = reader::read(&path)?;
    // OMP 18.18.8 records the parent's session file in the child's header.
    if let Some(recorded) = snapshot.header["parentSession"].as_str()
        && std::fs::canonicalize(recorded).ok() != std::fs::canonicalize(&locator.parent_file).ok()
    {
        return Err(AgentSessionError::Rejected);
    }
    if !Workspace::new(cwd).matches(&snapshot.header["cwd"]) {
        return Err(AgentSessionError::Rejected);
    }
    let journal = Journal {
        path,
        header: snapshot.header,
        title: snapshot.title,
        entries: snapshot.entries,
        identity: handle.session_id.clone(),
        parent: Some(locator.parent),
    };
    let (mut history, _) = journal.project(client, handle)?;
    history.resume_metadata = handle.metadata.clone().unwrap_or_default();
    Ok(history)
}

#[cfg(test)]
mod tests;
