use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore, mpsc};
use tokio::task::JoinHandle;
use tokio_util::task::AbortOnDropHandle;

use super::ClaudeClient;
use provider_core::ports::agent_session::{AgentSessionError, AgentSessionSpec};
use provider_core::ports::diagnostics::{Category, Diagnostic};
use provider_native::diagnostics::Diagnostics;
use provider_native::process_group::{self, Signal};

/// Largest stdout frame or native transcript line. `--replay-user-messages` and the transcript
/// echo complete user inputs, including inline images, so the cap covers the largest accepted
/// prompt plus Claude's record envelope.
pub(super) const MAX_FRAME: usize =
    provider_core::protocol::prompt::MAX_PROMPT_BYTES + 2 * 1024 * 1024;
const MAX_EVENTS: usize = 128;
/// Bytes of decoded stdout frames buffered between the reader and the session, across the
/// channel and the event backlog. One maximal frame always fits; the reader waits for budget
/// like it waits for channel capacity, so the count limit alone cannot buffer gigabytes.
const MAX_QUEUED_BYTES: usize = 2 * MAX_FRAME;
// A frame larger than the whole budget could never be admitted and would stall the reader.
const _: () = assert!(MAX_FRAME <= MAX_QUEUED_BYTES);
const MAX_REPLIES: usize = 32;

/// One decoded stdout frame holding its share of [`MAX_QUEUED_BYTES`] until it is consumed.
#[derive(Debug)]
struct Record {
    value: Value,
    _budget: OwnedSemaphorePermit,
}

#[derive(Debug)]
pub(super) struct Transport {
    child: Child,
    pub(super) diagnostics: Diagnostics,
    _stderr: AbortOnDropHandle<()>,
    input: Arc<Mutex<ChildStdin>>,
    messages: mpsc::Receiver<Record>,
    reader: JoinHandle<()>,
    events: VecDeque<Record>,
    /// Request ids answered through `send`; `--replay-user-messages` echoes those replies.
    replies: VecDeque<Value>,
    sequence: u64,
    deadline: Duration,
    closed: bool,
    /// Owner-only MCP document read by the child; removed when the transport is dropped.
    _mcp_config: Option<tempfile::NamedTempFile>,
}

impl Transport {
    pub(super) fn spawn(
        client: &ClaudeClient,
        spec: &AgentSessionSpec,
        binding: Option<(&str, bool)>,
        output_schema: Option<&Value>,
    ) -> Result<Self, AgentSessionError> {
        Self::spawn_with_metadata(client, spec, binding, output_schema, false)
    }

    pub(super) fn spawn_metadata(
        client: &ClaudeClient,
        spec: &AgentSessionSpec,
        schema: &Value,
    ) -> Result<Self, AgentSessionError> {
        Self::spawn_with_metadata(client, spec, None, Some(schema), true)
    }

    fn spawn_with_metadata(
        client: &ClaudeClient,
        spec: &AgentSessionSpec,
        binding: Option<(&str, bool)>,
        output_schema: Option<&Value>,
        metadata: bool,
    ) -> Result<Self, AgentSessionError> {
        #[cfg(windows)]
        if client
            .program
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
            })
        {
            return Err(AgentSessionError::Unavailable);
        }
        let (mut command, mcp_config) = launch_command(client, spec, binding)?;
        if metadata {
            command.args([
                "--tools=",
                "--strict-mcp-config",
                "--mcp-config={\"mcpServers\":{}}",
                "--settings={\"disableAllHooks\":true}",
            ]);
        }
        if let Some(schema) = output_schema {
            command.arg(format!("--json-schema={schema}"));
        }
        let diagnostics = Diagnostics::new("claude");
        let mut child = command
            .spawn()
            .map_err(|error| diagnostics.spawn_error(&error))?;
        let input = Arc::new(Mutex::new(
            child.stdin.take().ok_or(AgentSessionError::Failed)?,
        ));
        let output = child.stdout.take().ok_or(AgentSessionError::Failed)?;
        let stderr = child.stderr.take().ok_or(AgentSessionError::Failed)?;
        let stderr_task = AbortOnDropHandle::new(tokio::spawn(diagnostics.clone().drain(stderr)));
        let reader_diagnostics = diagnostics.clone();
        let (sender, messages) = mpsc::channel(MAX_EVENTS);
        let budget = Arc::new(Semaphore::new(MAX_QUEUED_BYTES));
        let reader_input = input.clone();
        let deadline = client.deadline;
        let reader = tokio::spawn(async move {
            let mut output = BufReader::new(output);
            loop {
                let mut bytes = Vec::new();
                let read = (&mut output)
                    .take(MAX_FRAME as u64)
                    .read_until(b'\n', &mut bytes)
                    .await;
                if !matches!(read, Ok(1..)) || bytes.last() != Some(&b'\n') {
                    let category = if matches!(read, Ok(0)) {
                        Category::Closed
                    } else {
                        Category::Protocol
                    };
                    reader_diagnostics.observe(Diagnostic::new(category, "stdout"), "read");
                    break;
                }
                if bytes.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                let Ok(message) = serde_json::from_slice::<Value>(&bytes) else {
                    reader_diagnostics
                        .observe(Diagnostic::new(Category::Protocol, "stdout"), "decode");
                    break;
                };
                if message["type"] == "control_request"
                    && message["request"]["subtype"] != "can_use_tool"
                {
                    let reply = json!({"type":"control_response","response":{
                        "subtype":"error","request_id":message["request_id"],"error":"Unsupported provider interaction"}});
                    let _ = tokio::time::timeout(deadline, write(&reader_input, &reply)).await;
                    break;
                }
                let Ok(size) = u32::try_from(bytes.len()) else {
                    break;
                };
                let Ok(permit) = budget.clone().acquire_many_owned(size).await else {
                    break;
                };
                let record = Record {
                    value: message,
                    _budget: permit,
                };
                if sender.send(record).await.is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            diagnostics,
            _stderr: stderr_task,
            input,
            messages,
            reader,
            events: VecDeque::new(),
            replies: VecDeque::new(),
            sequence: 0,
            deadline,
            closed: false,
            _mcp_config: mcp_config,
        })
    }

    pub(super) async fn initialize(&mut self) -> Result<Value, AgentSessionError> {
        self.request(json!({"subtype":"initialize","hooks":{}}))
            .await
    }

    pub(super) async fn request(&mut self, request: Value) -> Result<Value, AgentSessionError> {
        if self.closed {
            return Err(AgentSessionError::Failed);
        }
        self.sequence += 1;
        let id = format!("ait-{}", self.sequence);
        let result = tokio::time::timeout(self.deadline, async {
            write(
                &self.input,
                &json!({"type":"control_request","request_id":id,"request":request}),
            )
            .await?;
            loop {
                let record = self
                    .messages
                    .recv()
                    .await
                    .ok_or(AgentSessionError::Failed)?;
                let message = &record.value;
                if self.is_replayed_reply(message) {
                    continue;
                }
                if message["type"] == "control_response" {
                    let response = &message["response"];
                    if response["request_id"] != id {
                        return Err(AgentSessionError::Failed);
                    }
                    return match response["subtype"].as_str() {
                        Some("success") => Ok(response
                            .get("response")
                            .cloned()
                            .unwrap_or_else(|| json!({}))),
                        Some("error") => {
                            self.diagnostics.observe_native(
                                &message["response"]["error"],
                                "rpc",
                                "control",
                            );
                            Err(AgentSessionError::Rejected)
                        }
                        _ => Err(AgentSessionError::Failed),
                    };
                }
                if self.events.len() >= MAX_EVENTS {
                    return Err(AgentSessionError::Failed);
                }
                self.events.push_back(record);
            }
        })
        .await;
        match self
            .diagnostics
            .settle(result, "control", &[AgentSessionError::Rejected])
        {
            Err(AgentSessionError::Failed) => {
                let _ = self.close().await;
                Err(AgentSessionError::Failed)
            }
            settled => settled,
        }
    }

    pub(super) async fn send(&mut self, message: &Value) -> Result<(), AgentSessionError> {
        if !self.closed
            && matches!(
                tokio::time::timeout(self.deadline, write(&self.input, message)).await,
                Ok(Ok(()))
            )
        {
            if message["type"] == "control_response" {
                if self.replies.len() == MAX_REPLIES {
                    self.replies.pop_front();
                }
                self.replies
                    .push_back(message["response"]["request_id"].clone());
            }
            return Ok(());
        }
        let _ = self.close().await;
        Err(AgentSessionError::Failed)
    }

    pub(super) fn poll(&mut self) -> Result<Option<Value>, AgentSessionError> {
        loop {
            let record = match self.events.pop_front() {
                Some(record) => record,
                None => match self.messages.try_recv() {
                    Ok(record) => record,
                    Err(mpsc::error::TryRecvError::Empty) if !self.closed => return Ok(None),
                    Err(_) => return Err(AgentSessionError::Failed),
                },
            };
            if !self.is_replayed_reply(&record.value) {
                return Ok(Some(record.value));
            }
        }
    }

    /// Consume the native echo of a reply this transport wrote.
    fn is_replayed_reply(&mut self, message: &Value) -> bool {
        if message["type"] != "control_response" {
            return false;
        }
        let id = &message["response"]["request_id"];
        let Some(index) = self.replies.iter().position(|reply| reply == id) else {
            return false;
        };
        self.replies.remove(index);
        true
    }

    pub(super) async fn close(&mut self) -> Result<(), AgentSessionError> {
        self.diagnostics.flush();
        self.closed = true;
        self.reader.abort();
        process_group::signal(self.child.id(), Signal::Kill);
        let _ = self.child.start_kill();
        tokio::time::timeout(Duration::from_secs(2), self.child.wait())
            .await
            .map_err(|_| AgentSessionError::Failed)?
            .map_err(|_| AgentSessionError::Failed)?;
        Ok(())
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        self.reader.abort();
        process_group::signal(self.child.id(), Signal::Kill);
    }
}

async fn write(input: &Mutex<ChildStdin>, value: &Value) -> Result<(), AgentSessionError> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| AgentSessionError::Failed)?;
    if bytes.len() >= 64 * 1024 * 1024 {
        return Err(AgentSessionError::Rejected);
    }
    bytes.push(b'\n');
    let mut input = input.lock().await;
    input
        .write_all(&bytes)
        .await
        .map_err(|_| AgentSessionError::Failed)?;
    input.flush().await.map_err(|_| AgentSessionError::Failed)
}

fn launch_command(
    client: &ClaudeClient,
    spec: &AgentSessionSpec,
    binding: Option<(&str, bool)>,
) -> Result<(Command, Option<tempfile::NamedTempFile>), AgentSessionError> {
    let config_dir = client.config_dir.clone();
    #[cfg(target_os = "linux")]
    let config_dir = if spec.confinement.is_some() {
        Some(
            provider_native::sandbox::linux::claude_config_dir(&|name| match (name, &config_dir) {
                ("CLAUDE_CONFIG_DIR", _) if client.config_dir_from_home => None,
                ("CLAUDE_CONFIG_DIR", Some(directory)) => directory.to_str().map(str::to_owned),
                _ => client.environment.variable(name),
            })
            .map_err(|_| AgentSessionError::Failed)?,
        )
    } else {
        config_dir
    };
    let mut command = provider_native::sandbox::command(
        &client.program,
        spec.confinement.as_ref(),
        provider_native::sandbox::CLAUDE_STATE,
        &|name| match (name, &config_dir) {
            ("CLAUDE_CONFIG_DIR", Some(directory)) => directory.to_str().map(str::to_owned),
            _ => client.environment.variable(name),
        },
    )?;
    command
        .args([
            "--print",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
            "--replay-user-messages",
            "--permission-prompt-tool",
            "stdio",
            "--setting-sources=user,project,local",
        ])
        .arg(format!(
            "--permission-mode={}",
            spec.config.mode_id.as_deref().unwrap_or("default")
        ))
        .current_dir(&spec.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command.args(provider_native::configuration::claude(
        &spec.config,
        spec.confinement.is_some(),
    )?);
    let mcp_config = provider_native::configuration::claude_mcp(&spec.config)
        .map(|document| mcp_config_file(&document))
        .transpose()?;
    if let Some(file) = &mcp_config {
        command.arg("--mcp-config").arg(file.path());
    }
    command.envs(client.environment.entries());
    if let Some(path) = &config_dir {
        command.env("CLAUDE_CONFIG_DIR", path);
    }
    command
        .env_remove("CLAUDECODE")
        .env("CLAUDE_CODE_ENTRYPOINT", "ait")
        .env("CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING", "true");
    if let Some((id, resume)) = binding {
        command.arg(format!(
            "--{}={id}",
            if resume { "resume" } else { "session-id" }
        ));
    } else {
        command.arg("--no-session-persistence");
    }
    if let Some(model) = &spec.config.model {
        command.arg(format!("--model={model}"));
    }
    if let Some(effort) = &spec.config.thinking_option_id {
        match effort.as_str() {
            "off" => {
                command.arg("--thinking=disabled");
            }
            "ultracode" => {
                command.args(["--thinking=adaptive", "--effort=xhigh"]);
            }
            _ => {
                command.arg(format!("--effort={effort}"));
            }
        }
    }
    if let Some(prompt) = &spec.config.system_prompt {
        command.arg(format!("--append-system-prompt={prompt}"));
    }
    if spec.config.mode_id.as_deref() == Some("bypassPermissions") {
        command.arg("--allow-dangerously-skip-permissions");
    }
    #[cfg(unix)]
    command.process_group(0);
    Ok((command, mcp_config))
}

/// Write `document` to a new owner-only temporary file so MCP credentials stay out of `ps`.
fn mcp_config_file(document: &Value) -> Result<tempfile::NamedTempFile, AgentSessionError> {
    let mut file = tempfile::Builder::new()
        .prefix("ait-claude-mcp-")
        .suffix(".json")
        .tempfile()
        .map_err(|_| AgentSessionError::Failed)?;
    serde_json::to_writer(&mut file, document).map_err(|_| AgentSessionError::Failed)?;
    file.as_file()
        .sync_all()
        .map_err(|_| AgentSessionError::Failed)?;
    Ok(file)
}
