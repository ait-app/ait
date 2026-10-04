//! Translation from AIT Agent events into neutral `bonsai.session/1` bodies.
//!
//! Timeline input must be either *unmerged* rows (live `agent_stream` timeline events or
//! `Timeline::read` rows: progress deltas and completed suffixes, appended as they come) or a
//! whole generation read from zero. Never feed the merged display projection of
//! `agent.timeline.get` into an existing log: its entries repeat text already streamed.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::event::{
    AskKind, AskOption, Body, Choice, ContextUsage, Effect, Level, Origin, Question, TodoItem,
    TodoState, ToolKind, ToolState, chunks, field,
};
use crate::wire::{Answer, Person, display, is_token, truncate};

/// Text shown instead of an image or other binary output.
pub const IMAGE_PLACEHOLDER: &str = "[图片,在执行端本机]";
/// Resolution AIT publishes when the provider withdraws a request itself.
pub const NATIVE_WITHDRAWAL: &str = "Resolved by native provider";
const TOOL_TRUNCATION_MARKER: &str = "[Output truncated;";
/// Claude tools shown as `todo` or as a plan `ask` instead of a tool call.
const CLAUDE_HIDDEN_TOOLS: [&str; 5] = [
    "TodoWrite",
    "TaskCreate",
    "TaskUpdate",
    "TaskList",
    "ExitPlanMode",
];

/// Normalize a message ID for echo comparison: hex digits only, lowercase.
#[must_use]
pub fn normalize_id(id: &str) -> String {
    id.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

/// Write a 32-digit hex ID as a lowercase hyphenated UUID; other IDs are hashed into one.
#[must_use]
pub fn uuid_of(hex: &str) -> String {
    let digits: String = if hex.len() == 32 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        hex.to_ascii_lowercase()
    } else {
        sha256_hex(hex, 16)
    };
    format!(
        "{}-{}-{}-{}-{}",
        &digits[0..8],
        &digits[8..12],
        &digits[12..16],
        &digits[16..20],
        &digits[20..32]
    )
}

/// A contract ID (`[A-Za-z0-9._:-]{1,128}`) for a native one: kept when it fits, otherwise
/// `x-` plus a hash (stable, so every update of an item keeps its ID); `fallback` when the
/// native ID is missing or empty (the run page drops entries with empty IDs).
#[must_use]
pub fn contract_id(native: Option<&str>, fallback: &str) -> String {
    match native.filter(|native| !native.is_empty()) {
        Some(native) if is_token(native, 128) => native.to_owned(),
        Some(native) => format!("x-{}", hex_digest(native)),
        None => fallback.to_owned(),
    }
}

/// The `ask` ID for a native request ID (protocol §6: remapped when it does not fit).
#[must_use]
pub fn ask_id(native: &str) -> String {
    contract_id(Some(native), "ask")
}

fn hex_digest(text: &str) -> String {
    sha256_hex(text, 16)
}

/// The first `bytes` bytes of a text's SHA-256, as lowercase hex.
#[must_use]
pub fn sha256_hex(text: &str, bytes: usize) -> String {
    use sha2::Digest;
    use std::fmt::Write;
    sha2::Sha256::digest(text.as_bytes())
        .iter()
        .take(bytes)
        .fold(String::with_capacity(bytes * 2), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// How to answer one request, stored with it so answers work after a restart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskSpec {
    /// Request ID as sent to Bonsai.
    pub id: String,
    /// AIT's request ID when it differs from `id` (it did not fit the contract's pattern).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_id: Option<String>,
    /// Provider that asked.
    pub provider: String,
    /// Request kind.
    pub kind: AskKind,
    /// One-line title.
    pub title: String,
    /// What is being approved.
    pub detail: Option<String>,
    /// Whether `detail` was cut.
    pub truncated: bool,
    /// Offered options.
    pub options: Vec<AskOption>,
    /// Questions, for `kind: question`.
    pub questions: Option<Vec<Question>>,
    /// Native answer key per question key (question text for Claude, ID for Codex).
    pub answer_keys: Vec<(String, String)>,
    /// Native action IDs, so answers echo only IDs the provider offered.
    pub native_actions: Vec<String>,
}

impl AskSpec {
    /// AIT's request ID, for `agent.permission.resolve` and snapshot comparisons.
    #[must_use]
    pub fn native_id(&self) -> &str {
        self.native_id.as_deref().unwrap_or(&self.id)
    }

    /// The `ask` event body for this request.
    #[must_use]
    pub fn body(&self) -> Body {
        Body::Ask {
            id: self.id.clone(),
            kind: self.kind,
            title: self.title.clone(),
            detail: self.detail.clone(),
            truncated: self.truncated.then_some(true),
            options: self.options.clone(),
            questions: self.questions.clone(),
        }
    }

    /// Build AIT's permission response for an answer.
    ///
    /// # Returns
    ///
    /// The response and the effect, or a human-readable reason the answer is unusable.
    ///
    /// # Errors
    ///
    /// Returns a message for an unknown option, a missing or oversized answer.
    pub fn response(&self, answer: &Answer) -> Result<(Value, Effect), String> {
        let option = self
            .options
            .iter()
            .find(|option| option.id == answer.option_id)
            .ok_or_else(|| format!("没有这个选项:{}", answer.option_id))?;
        let selected = self
            .native_actions
            .contains(&option.id)
            .then(|| Value::from(option.id.clone()));
        let mut response = Map::new();
        match option.effect {
            Effect::Deny => {
                response.insert("behavior".to_owned(), Value::from("deny"));
                if let Some(selected) = selected {
                    response.insert("selectedActionId".to_owned(), selected);
                }
                if let Some(note) = answer.note.as_deref().filter(|note| !note.is_empty()) {
                    response.insert("message".to_owned(), Value::from(note));
                }
                response.insert("interrupt".to_owned(), Value::Bool(false));
            }
            Effect::Allow => {
                response.insert("behavior".to_owned(), Value::from("allow"));
                if let Some(selected) = selected {
                    response.insert("selectedActionId".to_owned(), selected);
                }
                if self.kind == AskKind::Question {
                    response.insert("updatedInput".to_owned(), self.answers(answer)?);
                }
            }
        }
        Ok((Value::Object(response), option.effect))
    }

    fn answers(&self, answer: &Answer) -> Result<Value, String> {
        let given = answer.answers.clone().unwrap_or_default();
        let mut native = Map::new();
        for (key, native_key) in &self.answer_keys {
            let values: Vec<String> = given
                .get(key)
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(Value::as_str)
                        .filter(|value| !value.is_empty())
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            if values.is_empty() {
                return Err(format!("问题 {key} 没有回答"));
            }
            if values.iter().any(|value| value.len() > 4096) {
                return Err(format!("问题 {key} 的回答超过 4 KiB"));
            }
            let value = if self.provider == "claude" {
                let joined = values.join(", ");
                if joined.len() > 4096 {
                    return Err(format!("问题 {key} 的回答超过 4 KiB"));
                }
                Value::from(joined)
            } else {
                Value::from(values)
            };
            native.insert(native_key.clone(), value);
        }
        Ok(json!({ "answers": native }))
    }
}

/// Stateful translator for one run: knows which inputs it sent and who owns the machine.
#[derive(Debug, Default, Clone)]
pub struct Translator {
    own_inputs: HashSet<String>,
    restored: HashMap<String, Body>,
    owner: Option<Person>,
    file_paths: HashMap<String, String>,
    diffs: HashMap<String, String>,
}

impl Translator {
    /// Create a translator attributing local input to `owner`.
    #[must_use]
    pub fn new(owner: Option<Person>) -> Self {
        Self {
            owner,
            ..Self::default()
        }
    }

    /// Remember an input this adapter sent so its echo is dropped.
    pub fn sent(&mut self, message_id: &str) {
        self.own_inputs.insert(normalize_id(message_id));
    }

    /// While rebuilding a log from a whole generation, show each echo of an input this adapter
    /// sent as the original `input` event (with its ID and sender) instead of dropping it.
    pub fn restore_inputs(&mut self, inputs: Vec<(String, Body)>) {
        self.restored = inputs
            .into_iter()
            .map(|(message_id, body)| (normalize_id(&message_id), body))
            .collect();
    }

    /// Stop restoring echoes (live translation drops them again).
    pub fn clear_restored(&mut self) {
        self.restored.clear();
    }

    /// Update the machine owner after a reconnect.
    pub fn set_owner(&mut self, owner: Person) {
        self.owner = Some(owner);
    }

    /// The machine owner, when known.
    #[must_use]
    pub fn owner(&self) -> Option<&Person> {
        self.owner.as_ref()
    }

    /// Translate one timeline item.
    #[must_use]
    pub fn item(&mut self, provider: &str, item: &Value) -> Vec<Body> {
        match item.get("type").and_then(Value::as_str) {
            Some("assistant_message") => assistant(item),
            Some("reasoning") => reasoning(item),
            Some("tool_call") => self.tool(provider, item).into_iter().collect(),
            Some("todo") => vec![todo(item)],
            Some("user_message") => self.user_message(item).into_iter().collect(),
            Some("notification") => vec![notification(item)],
            Some("error") => vec![error(item)],
            Some("compaction") => compaction(item).into_iter().collect(),
            Some(other) => vec![Body::Notice {
                level: Level::Info,
                text: format!("执行端有一条这里显示不了的记录({})", display(other, 64)),
            }],
            None => Vec::new(),
        }
    }

    fn user_message(&self, item: &Value) -> Option<Body> {
        let message_id = text_of(item, "messageId")?;
        let text = text_of(item, "text").unwrap_or_default();
        for key in ["clientMessageId", "messageId"] {
            if let Some(restored) =
                text_of(item, key).and_then(|id| self.restored.get(&normalize_id(id)))
            {
                return Some(restored.clone());
            }
        }
        let own = |key: &str| {
            text_of(item, key).is_some_and(|id| self.own_inputs.contains(&normalize_id(id)))
        };
        if own("clientMessageId") || own("messageId") {
            return None;
        }
        let (by, login) = self.owner.as_ref().map_or_else(
            || ("runtime:local".to_owned(), None),
            |owner| (owner.id.clone(), owner.login.clone()),
        );
        Some(Body::Input {
            id: contract_id(Some(message_id), &format!("u-{}", hex_digest(text))),
            text: text.to_owned(),
            by,
            login,
            origin: Origin::User,
        })
    }

    fn tool(&mut self, provider: &str, item: &Value) -> Option<Body> {
        let name = text_of(item, "name").unwrap_or("tool");
        if provider == "claude" && CLAUDE_HIDDEN_TOOLS.contains(&name) {
            return None;
        }
        let id = text_of(item, "callId")
            .filter(|id| !id.is_empty())?
            .to_owned();
        let state = match text_of(item, "status") {
            Some("running") => ToolState::Running,
            Some("completed") => ToolState::Ok,
            Some("failed") => ToolState::Failed,
            _ => ToolState::Ended,
        };
        let empty = Map::new();
        let detail = item
            .get("detail")
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        let view = tool_view(name, detail);
        if let Some(path) = detail.get("filePath").and_then(Value::as_str) {
            self.file_paths.insert(id.clone(), path.to_owned());
        }
        if let Some(diff) = detail.get("unifiedDiff").and_then(Value::as_str) {
            self.diffs.insert(id.clone(), diff.to_owned());
        }
        let running_tail = provider == "codex" && state == ToolState::Running;
        let (input, input_cut) = cut(view.input.as_deref());
        let (output, output_cut) = cut(view.output.as_deref());
        let marked = view
            .output
            .as_deref()
            .is_some_and(|output| output.contains(TOOL_TRUNCATION_MARKER));
        let cut_any = input_cut || output_cut || marked;
        Some(Body::Tool {
            id: contract_id(Some(&id), "tool"),
            name: name.to_owned(),
            state,
            kind: Some(view.kind),
            title: view.title.map(|title| one_line(&title)),
            input,
            output,
            truncated: if running_tail && !cut_any {
                None
            } else {
                Some(cut_any)
            },
        })
    }

    /// Translate a `permission_requested` request into a stored specification.
    #[must_use]
    pub fn ask(&self, request: &Value) -> Option<AskSpec> {
        let native_id = text_of(request, "id").filter(|id| !id.is_empty())?;
        let id = ask_id(native_id);
        let provider = text_of(request, "provider").unwrap_or("claude").to_owned();
        let name = text_of(request, "name").unwrap_or("");
        let kind = match (text_of(request, "kind"), name) {
            (Some("question"), _) => AskKind::Question,
            (Some("plan"), _) | (_, "CodexPlanApproval" | "ExitPlanMode") => AskKind::Plan,
            _ => AskKind::Tool,
        };
        let empty = Map::new();
        let input = request
            .get("input")
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        let (questions, answer_keys) = if kind == AskKind::Question {
            questions(&provider, input)
        } else {
            (None, Vec::new())
        };
        let (options, native_actions) = options(&provider, kind, request);
        let raw_detail = self.ask_detail(&provider, name, kind, request, input);
        let (detail, truncated) = match raw_detail.filter(|detail| !detail.is_empty()) {
            Some(detail) => {
                let (detail, cut) = field(&detail);
                (Some(detail), cut)
            }
            None => (None, false),
        };
        let title = ask_title(&provider, name, kind, request, input, &self.file_paths);
        let detail = match (kind, detail) {
            (AskKind::Tool, None) => Some(title.clone()),
            (_, detail) => detail,
        };
        Some(AskSpec {
            native_id: (id != native_id).then(|| native_id.to_owned()),
            id,
            provider,
            kind,
            title,
            detail,
            truncated,
            options,
            questions,
            answer_keys,
            native_actions,
        })
    }

    fn ask_detail(
        &self,
        provider: &str,
        name: &str,
        kind: AskKind,
        request: &Value,
        input: &Map<String, Value>,
    ) -> Option<String> {
        if kind == AskKind::Plan {
            return input.get("plan").and_then(Value::as_str).map(str::to_owned);
        }
        if kind == AskKind::Question {
            return None;
        }
        if provider == "codex" {
            return match name {
                "commandExecution" => {
                    let mut lines = Vec::new();
                    if let Some(command) = command_text(input.get("command")) {
                        lines.push(command);
                    }
                    if let Some(reason) = input.get("reason").and_then(Value::as_str) {
                        lines.push(format!("reason: {reason}"));
                    }
                    Some(lines.join("\n"))
                }
                "fileChange" => {
                    let change = input
                        .get("itemId")
                        .and_then(Value::as_str)
                        .and_then(|item| self.diffs.get(item).cloned())
                        .unwrap_or_else(|| pretty(&Value::Object(input.clone())));
                    match input.get("grantRoot").filter(|root| !root.is_null()) {
                        Some(root) => Some(format!("grantRoot: {}\n{change}", text_or_json(root))),
                        None => Some(change),
                    }
                }
                _ => request
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| Some(pretty(&Value::Object(input.clone())))),
            };
        }
        match name {
            "Bash" => input
                .get("command")
                .and_then(Value::as_str)
                .map(str::to_owned),
            "Edit" => Some(edit_diff(input)),
            "Write" => {
                let path = input.get("file_path").and_then(Value::as_str).unwrap_or("");
                let content = input.get("content").and_then(Value::as_str).unwrap_or("");
                Some(format!("{path}\n{content}"))
            }
            _ => Some(pretty(&Value::Object(input.clone()))),
        }
    }
}

struct ToolView {
    kind: ToolKind,
    title: Option<String>,
    input: Option<String>,
    output: Option<String>,
}

fn tool_view(name: &str, detail: &Map<String, Value>) -> ToolView {
    let get = |key: &str| detail.get(key).and_then(Value::as_str).map(str::to_owned);
    match detail.get("type").and_then(Value::as_str) {
        Some("shell") => {
            let command = get("command").unwrap_or_default();
            let input = match get("cwd") {
                Some(cwd) => format!("{command}\n(cwd: {cwd})"),
                None => command.clone(),
            };
            let output = match (
                get("output"),
                detail.get("exitCode").and_then(Value::as_i64),
            ) {
                (Some(output), Some(code)) => Some(format!("{output}\nexit code: {code}")),
                (output, Some(code)) => output.or_else(|| Some(format!("exit code: {code}"))),
                (output, None) => output,
            };
            ToolView {
                kind: ToolKind::Shell,
                title: Some(command),
                input: Some(input),
                output,
            }
        }
        Some("read") => {
            let path = get("filePath").unwrap_or_default();
            let mut input = path.clone();
            for key in ["offset", "limit"] {
                if let Some(value) = detail.get(key).filter(|value| !value.is_null()) {
                    use std::fmt::Write;
                    let _ = write!(input, "\n{key}: {value}");
                }
            }
            ToolView {
                kind: ToolKind::Read,
                title: Some(path),
                input: Some(input),
                output: get("content"),
            }
        }
        Some("edit") => ToolView {
            kind: ToolKind::Edit,
            title: get("filePath"),
            input: get("unifiedDiff").or_else(|| Some(edit_diff(detail))),
            output: None,
        },
        Some("write") => ToolView {
            kind: ToolKind::Write,
            title: get("filePath"),
            input: get("content"),
            output: None,
        },
        Some("search") => ToolView {
            kind: ToolKind::Search,
            title: get("query"),
            input: get("query"),
            output: get("content"),
        },
        Some("fetch") => ToolView {
            kind: ToolKind::Fetch,
            title: get("url"),
            input: get("url"),
            output: get("result"),
        },
        Some("sub_agent") => ToolView {
            kind: ToolKind::Agent,
            title: get("description").or_else(|| get("subAgentType")),
            input: get("prompt"),
            output: get("log").filter(|log| !log.is_empty()),
        },
        Some("plan") => ToolView {
            kind: ToolKind::Plan,
            title: Some("计划".to_owned()),
            input: get("text"),
            output: None,
        },
        Some("plain_text") => ToolView {
            kind: ToolKind::Other,
            title: get("label").or_else(|| Some(name.to_owned())),
            input: (get("label").as_deref() == Some("Image")).then(|| IMAGE_PLACEHOLDER.to_owned()),
            output: None,
        },
        Some("unknown") if is_mcp(name) => mcp_view(name, detail),
        // Unknown detail types: never forward fields that look like local paths (adapter §4.3).
        _ => ToolView {
            kind: ToolKind::Other,
            title: Some(name.to_owned()),
            input: detail
                .get("input")
                .filter(|value| !value.is_null())
                .map(|value| text_or_json(&without_paths(value))),
            output: detail
                .get("output")
                .filter(|value| !value.is_null())
                .map(|value| text_or_json(&without_paths(value))),
        },
    }
}

/// A JSON value without keys that name local paths (`path`, `filePath`, `cwd`, `root`, …).
fn without_paths(value: &Value) -> Value {
    match value {
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .filter(|(key, _)| !is_path_key(key))
                .map(|(key, value)| (key.clone(), without_paths(value)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(without_paths).collect()),
        other => other.clone(),
    }
}

fn is_path_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase().replace(['_', '-'], "");
    let singular = lower.strip_suffix('s').unwrap_or(&lower);
    [
        "path",
        "root",
        "dir",
        "directory",
        "directorie",
        "folder",
        "cwd",
        "home",
    ]
    .iter()
    .any(|suffix| singular.ends_with(suffix))
}

/// Whether a tool row is an MCP call: Claude's `mcp__server__tool`, an AIT `mcpToolCall` row,
/// or a Codex `server.tool` row.
fn is_mcp(name: &str) -> bool {
    name.starts_with("mcp__") || name == "mcpToolCall" || server_tool(name).is_some()
}

/// Split a Codex MCP row's `server.tool` name; AIT 0.0.16 names Codex MCP calls this way.
fn server_tool(name: &str) -> Option<(&str, &str)> {
    name.split_once('.')
        .filter(|(server, tool)| !server.is_empty() && !tool.is_empty())
}

fn mcp_view(name: &str, detail: &Map<String, Value>) -> ToolView {
    let input = detail.get("input");
    let (server, tool, arguments) =
        if let Some((server, tool)) = server_tool(name).filter(|_| !name.starts_with("mcp__")) {
            (server.to_owned(), tool.to_owned(), input.cloned())
        } else if name == "mcpToolCall" {
            let native = input.and_then(Value::as_object);
            let field = |key: &str| {
                native
                    .and_then(|native| native.get(key))
                    .and_then(Value::as_str)
                    .unwrap_or("?")
                    .to_owned()
            };
            (
                field("server"),
                field("tool"),
                native.and_then(|native| native.get("arguments")).cloned(),
            )
        } else {
            let rest = name.trim_start_matches("mcp__");
            let (server, tool) = rest.split_once("__").unwrap_or((rest, ""));
            (server.to_owned(), tool.to_owned(), input.cloned())
        };
    ToolView {
        kind: ToolKind::Mcp,
        title: Some(format!("{server} · {tool}")),
        input: arguments
            .filter(|value| !value.is_null())
            .map(|value| pretty(&value)),
        output: detail
            .get("output")
            .filter(|value| !value.is_null())
            .map(text_or_json),
    }
}

fn assistant(item: &Value) -> Vec<Body> {
    let Some(text) = text_of(item, "text").filter(|text| !text.is_empty()) else {
        return Vec::new();
    };
    let mid = contract_id(text_of(item, "messageId"), "assistant");
    let text = if is_image(text) {
        IMAGE_PLACEHOLDER
    } else {
        text
    };
    chunks(text)
        .into_iter()
        .map(|part| Body::Text {
            mid: mid.clone(),
            text: part.to_owned(),
        })
        .collect()
}

fn is_image(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.starts_with("![Image](") && trimmed.ends_with(')') && !trimmed.contains('\n')
}

fn reasoning(item: &Value) -> Vec<Body> {
    let Some(text) = text_of(item, "text").filter(|text| !text.is_empty()) else {
        return Vec::new();
    };
    chunks(text)
        .into_iter()
        .map(|part| Body::Reasoning {
            mid: None,
            text: part.to_owned(),
        })
        .collect()
}

fn todo(item: &Value) -> Body {
    let items = item
        .get("items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|entry| TodoItem {
                    text: text_of(entry, "text").unwrap_or_default().to_owned(),
                    state: match text_of(entry, "status") {
                        Some("in_progress") => TodoState::Active,
                        Some("completed") => TodoState::Done,
                        _ => TodoState::Pending,
                    },
                })
                .collect()
        })
        .unwrap_or_default();
    Body::Todo { items }
}

fn notification(item: &Value) -> Body {
    Body::Notice {
        level: match text_of(item, "level") {
            Some("warning") => Level::Warning,
            Some("error") => Level::Error,
            _ => Level::Info,
        },
        text: text_of(item, "message").unwrap_or_default().to_owned(),
    }
}

/// A provider error row, such as Codex reporting that the selected model is at capacity.
fn error(item: &Value) -> Body {
    Body::Notice {
        level: Level::Error,
        text: text_of(item, "message")
            .filter(|message| !message.is_empty())
            .unwrap_or("执行端报告了一个错误")
            .to_owned(),
    }
}

fn compaction(item: &Value) -> Option<Body> {
    (text_of(item, "status") == Some("completed")).then(|| Body::Notice {
        level: Level::Info,
        text: "上下文已压缩(Context compacted)".to_owned(),
    })
}

/// Translate an AIT usage snapshot.
#[must_use]
pub fn usage(usage: &Value) -> Option<Body> {
    let number = |key: &str| usage.get(key).and_then(Value::as_u64);
    let context = match (
        number("contextWindowUsedTokens"),
        number("contextWindowMaxTokens"),
    ) {
        (Some(used), Some(max)) => Some(ContextUsage { used, max }),
        _ => None,
    };
    let input = number("inputTokens");
    let output = number("outputTokens");
    if input.is_none() && output.is_none() && context.is_none() {
        return None;
    }
    Some(Body::Usage {
        input,
        output,
        context,
    })
}

/// Whether a `permission_resolved` resolution is the provider withdrawing the request.
#[must_use]
pub fn is_withdrawal(resolution: &Value) -> bool {
    *resolution == json!({"behavior": "deny", "message": NATIVE_WITHDRAWAL})
}

/// The effect of a resolution the machine owner gave in AIT.
#[must_use]
pub fn effect_of(resolution: &Value) -> Effect {
    if resolution.get("behavior").and_then(Value::as_str) == Some("allow") {
        Effect::Allow
    } else {
        Effect::Deny
    }
}

fn options(provider: &str, kind: AskKind, request: &Value) -> (Vec<AskOption>, Vec<String>) {
    let actions: Vec<&Map<String, Value>> = request
        .get("actions")
        .and_then(Value::as_array)
        .map(|actions| actions.iter().filter_map(Value::as_object).collect())
        .unwrap_or_default();
    let native: Vec<String> = actions
        .iter()
        .filter_map(|action| action.get("id").and_then(Value::as_str).map(str::to_owned))
        .collect();
    let suggestions = request.get("suggestions").and_then(Value::as_array);
    let root = request
        .get("input")
        .and_then(|input| input.get("grantRoot"))
        .filter(|root| !root.is_null())
        .map(text_or_json);
    let grant_root = root.is_some();
    let mut options = Vec::new();
    for action in &actions {
        let Some(id) = action.get("id").and_then(Value::as_str) else {
            continue;
        };
        let effect = match action.get("behavior").and_then(Value::as_str) {
            Some("allow") => Effect::Allow,
            Some("deny") => Effect::Deny,
            _ => continue,
        };
        let label = match (provider, id) {
            (_, "allow") if kind == AskKind::Question => "提交".to_owned(),
            (_, "deny") if kind == AskKind::Question => "不回答".to_owned(),
            ("codex", "allow") if grant_root => continue,
            (_, "allow") => "允许这一次".to_owned(),
            (_, "deny") => "拒绝".to_owned(),
            ("claude", _) if id.starts_with("allow-update-") => {
                let Some(rule) = session_rule(suggestions, id) else {
                    continue;
                };
                format!("本次会话里都允许:{rule}")
            }
            // Approving a grantRoot request widens the sandbox: say to what.
            ("codex", "allow-session") => match &root {
                Some(root) => one_line(&format!("本次会话里都允许写入 {root}")),
                None => "本次会话里都允许".to_owned(),
            },
            ("codex", "implement") => "按计划开始".to_owned(),
            ("codex", "dismiss") => "不采用".to_owned(),
            _ => continue,
        };
        if kind == AskKind::Question && id != "allow" && id != "deny" {
            continue;
        }
        if is_token(id, 128) {
            options.push(AskOption {
                id: id.to_owned(),
                label,
                effect,
            });
        }
    }
    // A codex grantRoot request accepts only a session grant; AIT refuses a plain allow.
    if !grant_root && !options.iter().any(|option| option.effect == Effect::Allow) {
        options.insert(
            0,
            AskOption {
                id: "allow".to_owned(),
                label: if kind == AskKind::Question {
                    "提交"
                } else {
                    "允许这一次"
                }
                .to_owned(),
                effect: Effect::Allow,
            },
        );
    }
    if !options.iter().any(|option| option.effect == Effect::Deny) {
        options.push(AskOption {
            id: "deny".to_owned(),
            label: if kind == AskKind::Question {
                "不回答"
            } else {
                "拒绝"
            }
            .to_owned(),
            effect: Effect::Deny,
        });
    }
    (options, native)
}

fn session_rule(suggestions: Option<&Vec<Value>>, id: &str) -> Option<String> {
    let index: usize = id.trim_start_matches("allow-update-").parse().ok()?;
    let suggestion = suggestions?.get(index)?;
    if suggestion.get("destination").and_then(Value::as_str) != Some("session") {
        return None;
    }
    let rules = suggestion
        .get("rules")
        .and_then(Value::as_array)
        .map(|rules| {
            rules
                .iter()
                .map(|rule| {
                    let tool = rule.get("toolName").and_then(Value::as_str).unwrap_or("");
                    match rule.get("ruleContent").and_then(Value::as_str) {
                        Some(content) => format!("{tool}({content})"),
                        None => tool.to_owned(),
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    Some(one_line(&rules))
}

fn questions(
    provider: &str,
    input: &Map<String, Value>,
) -> (Option<Vec<Question>>, Vec<(String, String)>) {
    let Some(native) = input.get("questions").and_then(Value::as_array) else {
        return (None, Vec::new());
    };
    let mut questions = Vec::new();
    let mut keys = Vec::new();
    for (index, entry) in native.iter().enumerate() {
        let question = text_of(entry, "question").unwrap_or_default().to_owned();
        let header = text_of(entry, "header").map(str::to_owned);
        let (key, native_key) = if provider == "claude" {
            (format!("q{index}"), question.clone())
        } else {
            let native = text_of(entry, "id");
            (
                contract_id(native, &format!("q{index}")),
                native.map_or_else(|| format!("q{index}"), str::to_owned),
            )
        };
        let choices = entry
            .get("options")
            .and_then(Value::as_array)
            .map(|options| {
                options
                    .iter()
                    .filter_map(|option| {
                        Some(Choice {
                            label: text_of(option, "label")?.to_owned(),
                            description: text_of(option, "description").map(str::to_owned),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let flag = |key: &str| entry.get(key).and_then(Value::as_bool).unwrap_or(false);
        questions.push(Question {
            key: key.clone(),
            header,
            question,
            options: choices,
            multi: flag("multiSelect"),
            other: provider == "claude" || flag("isOther") || flag("allowOther"),
        });
        keys.push((key, native_key));
    }
    (Some(questions), keys)
}

fn ask_title(
    provider: &str,
    name: &str,
    kind: AskKind,
    request: &Value,
    input: &Map<String, Value>,
    file_paths: &HashMap<String, String>,
) -> String {
    let first_question = || {
        input
            .get("questions")
            .and_then(Value::as_array)
            .and_then(|questions| questions.first())
            .and_then(|question| {
                text_of(question, "header").or_else(|| text_of(question, "question"))
            })
            .map(str::to_owned)
    };
    let title = match kind {
        AskKind::Plan => Some("计划".to_owned()),
        AskKind::Question => {
            first_question().or_else(|| text_of(request, "title").map(str::to_owned))
        }
        AskKind::Tool if provider == "codex" => match name {
            "commandExecution" => command_text(input.get("command")),
            "fileChange" => input
                .get("itemId")
                .and_then(Value::as_str)
                .and_then(|item| file_paths.get(item).cloned())
                .or_else(|| Some("改文件".to_owned())),
            _ => text_of(request, "title").map(str::to_owned),
        },
        AskKind::Tool => {
            let summary = match name {
                "Bash" => input
                    .get("command")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                "Edit" | "Write" | "Read" => input
                    .get("file_path")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                _ => None,
            };
            Some(summary.map_or_else(|| name.to_owned(), |summary| format!("{name}: {summary}")))
        }
    };
    let title = title.map(|title| one_line(&title)).unwrap_or_default();
    if title.is_empty() {
        if name.is_empty() {
            "请求批准".to_owned()
        } else {
            one_line(name)
        }
    } else {
        title
    }
}

fn command_text(command: Option<&Value>) -> Option<String> {
    match command? {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => Some(
            parts
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" "),
        ),
        _ => None,
    }
}

fn edit_diff(fields: &Map<String, Value>) -> String {
    let path = fields
        .get("filePath")
        .or_else(|| fields.get("file_path"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let old = fields
        .get("oldString")
        .or_else(|| fields.get("old_string"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let new = fields
        .get("newString")
        .or_else(|| fields.get("new_string"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let mut diff = format!("--- {path}\n+++ {path}\n");
    for line in old.lines() {
        diff.push('-');
        diff.push_str(line);
        diff.push('\n');
    }
    for line in new.lines() {
        diff.push('+');
        diff.push_str(line);
        diff.push('\n');
    }
    diff
}

fn one_line(text: &str) -> String {
    let first = text.lines().next().unwrap_or("");
    display(first, 200)
}

fn cut(text: Option<&str>) -> (Option<String>, bool) {
    match text {
        Some(text) => {
            let (prefix, was_cut) = field(text);
            (Some(prefix), was_cut)
        }
        None => (None, false),
    }
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

fn text_or_json(value: &Value) -> String {
    value.as_str().map_or_else(|| pretty(value), str::to_owned)
}

fn text_of<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// Cut a final assistant text to the status limit, on a code point.
#[must_use]
pub fn final_text(text: &str) -> Option<String> {
    let (prefix, _) = truncate(text, crate::wire::FINAL_TEXT_BYTES);
    (!prefix.is_empty()).then(|| prefix.to_owned())
}

#[cfg(test)]
mod tests;
