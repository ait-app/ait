//! Own one DSH Web Host and its authenticated event socket for one Ait session.
use futures_util::{SinkExt, StreamExt, stream::SplitSink};
use serde_json::{Value, json};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    net::TcpStream,
    process::Child,
    sync::mpsc,
    task::JoinHandle,
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message, client::IntoClientRequest},
};

use super::super::DeepSeekHarnessClient;
use super::http::{Api, MAX_FRAME};
use provider_core::ports::agent_session::AgentSessionError;
use provider_native::process_group::{self, Signal};

type Writer = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>;

#[derive(Debug)]
pub(super) struct Runtime {
    pub(super) api: Api,
    pub(super) diagnostics: provider_native::diagnostics::Diagnostics,
    _stderr: tokio_util::task::AbortOnDropHandle<()>,
    pub(super) client_id: String,
    child: Child,
    reader: JoinHandle<()>,
    writer: Writer,
    events: mpsc::Receiver<Value>,
    closed: bool,
    /// Private MCP overlay applied to this Host, removed once the Host is stopped.
    overlay: Option<super::mcp::Overlay>,
}

impl Runtime {
    /// Start a private loopback Host without opening a browser and await its event readiness.
    pub(super) async fn open(
        client: &DeepSeekHarnessClient,
        cwd: &str,
    ) -> Result<Self, AgentSessionError> {
        Self::open_with(client, cwd, None).await
    }

    /// Start a Host like [`Self::open`], applying a session's private MCP `overlay`, if any.
    /// The overlay lives until the Host is stopped.
    pub(super) async fn open_with(
        client: &DeepSeekHarnessClient,
        cwd: &str,
        overlay: Option<super::mcp::Overlay>,
    ) -> Result<Self, AgentSessionError> {
        let diagnostics = provider_native::diagnostics::Diagnostics::new("deepseek-harness");
        let mut command = client.command();
        command.arg("--profile").arg(&client.native_profile);
        if let Some(overlay) = &overlay {
            command.args(overlay.arguments());
        }
        command
            .args(["--no-open", "--host", "127.0.0.1", "--port", "0"])
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command
            .spawn()
            .map_err(|error| diagnostics.spawn_error(&error))?;
        let stderr = child.stderr.take().ok_or(AgentSessionError::Failed)?;
        let stderr_task = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(
            diagnostics.clone().drain(stderr),
        ));
        let result = tokio::time::timeout(
            client.deadline,
            Self::connect(&mut child, client.deadline, diagnostics.clone()),
        )
        .await;
        if let Ok(Ok((api, writer, events, reader, client_id))) = result {
            Ok(Self {
                api,
                diagnostics,
                _stderr: stderr_task,
                writer,
                events,
                reader,
                client_id,
                child,
                closed: false,
                overlay,
            })
        } else {
            let category = if result.is_err() {
                provider_core::ports::diagnostics::Category::Timeout
            } else {
                provider_core::ports::diagnostics::Category::Protocol
            };
            diagnostics.observe(
                provider_core::ports::diagnostics::Diagnostic::new(category, "transport"),
                "connect",
            );
            process_group::signal(child.id(), Signal::Kill);
            let _ = child.kill().await;
            Err(AgentSessionError::Failed)
        }
    }

    async fn connect(
        child: &mut Child,
        deadline: Duration,
        diagnostics: provider_native::diagnostics::Diagnostics,
    ) -> Result<(Api, Writer, mpsc::Receiver<Value>, JoinHandle<()>, String), AgentSessionError>
    {
        let output = child.stdout.take().ok_or(AgentSessionError::Failed)?;
        let mut output = BufReader::new(output);
        let mut total = 0;
        let launch = loop {
            let mut line = Vec::new();
            let read = (&mut output)
                .take(8192)
                .read_until(b'\n', &mut line)
                .await
                .map_err(|_| AgentSessionError::Failed)?;
            total += read;
            if read == 0 || line.last() != Some(&b'\n') || total > 65536 {
                return Err(AgentSessionError::Failed);
            }
            let line = std::str::from_utf8(&line).map_err(|_| AgentSessionError::Failed)?;
            if let Some(url) = line.strip_prefix("dsh web: ") {
                break url.trim().to_owned();
            }
        };
        let api = Api::connect(&launch, deadline, diagnostics.clone()).await?;
        let mut url = api
            .base
            .join("api/remote.mux")
            .map_err(|_| AgentSessionError::Failed)?;
        url.set_scheme("ws")
            .map_err(|()| AgentSessionError::Failed)?;
        let mut request = url
            .as_str()
            .into_client_request()
            .map_err(|_| AgentSessionError::Failed)?;
        request.headers_mut().insert("cookie", api.cookie.clone());
        let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
            .max_message_size(Some(MAX_FRAME))
            .max_frame_size(Some(MAX_FRAME));
        let (mut socket, _) =
            tokio_tungstenite::connect_async_with_config(request, Some(config), false)
                .await
                .map_err(|_| AgentSessionError::Failed)?;
        socket.send(Message::Text(json!({"type":"open","streamId":"events","endpoint":"$events","payload":{"args":{}}}).to_string().into())).await.map_err(|_| AgentSessionError::Failed)?;
        let ready = socket
            .next()
            .await
            .ok_or(AgentSessionError::Failed)?
            .map_err(|_| AgentSessionError::Failed)?;
        let ready: Value =
            serde_json::from_str(ready.to_text().map_err(|_| AgentSessionError::Failed)?)
                .map_err(|_| AgentSessionError::Failed)?;
        if ready["type"] != "item"
            || ready["streamId"] != "events"
            || ready["value"]["type"] != "ready"
        {
            return Err(AgentSessionError::Failed);
        }
        let client_id = provider_native::acp_config::text(&ready["value"], "clientId")?.to_owned();
        let (writer, mut socket) = socket.split();
        let (sender, events) = mpsc::channel(128);
        let reader = tokio::spawn(async move {
            // Drain diagnostics without retaining or logging the authenticated startup URL.
            let drain = tokio::spawn(async move {
                let _ = tokio::io::copy(&mut output, &mut tokio::io::sink()).await;
            });
            while let Some(Ok(message)) = socket.next().await {
                if let Message::Text(text) = message {
                    let Ok(value) = serde_json::from_str(&text) else {
                        break;
                    };
                    if sender.send(value).await.is_err() {
                        break;
                    }
                } else if matches!(message, Message::Close(_) | Message::Binary(_)) {
                    break;
                }
            }
            diagnostics.observe(
                provider_core::ports::diagnostics::Diagnostic::new(
                    provider_core::ports::diagnostics::Category::Closed,
                    "transport",
                ),
                "events",
            );
            drain.abort();
        });
        Ok((api, writer, events, reader, client_id))
    }

    /// Subscribe to one native stream. The caller verifies its opening snapshot before input.
    pub(super) async fn subscribe(
        &mut self,
        id: &str,
        endpoint: &str,
        args: Value,
    ) -> Result<(), AgentSessionError> {
        self.writer
            .send(Message::Text(
                json!({"type":"open","streamId":id,"endpoint":endpoint,"payload":{"args":args}})
                    .to_string()
                    .into(),
            ))
            .await
            .map_err(|_| AgentSessionError::Failed)
    }

    /// Release one read-only native stream; returns an error if its socket is closed.
    pub(super) async fn unsubscribe(&mut self, id: &str) -> Result<(), AgentSessionError> {
        self.writer
            .send(Message::Text(
                json!({"type":"cancel","streamId":id}).to_string().into(),
            ))
            .await
            .map_err(|_| AgentSessionError::Failed)
    }

    /// Receive a bounded frame during initialization; timeout prevents stalled native controls.
    pub(super) async fn next(&mut self) -> Result<Value, AgentSessionError> {
        tokio::time::timeout(Duration::from_secs(30), self.events.recv())
            .await
            .map_err(|_| AgentSessionError::Failed)?
            .ok_or(AgentSessionError::Failed)
    }

    /// Poll one native observation, reporting a lost connection as uncertain execution.
    pub(super) fn poll(&mut self) -> Result<Option<Value>, AgentSessionError> {
        match self.events.try_recv() {
            Ok(value) => Ok(Some(value)),
            Err(mpsc::error::TryRecvError::Empty) if !self.closed => Ok(None),
            Err(_) => Err(AgentSessionError::Failed),
        }
    }

    /// Dispose the owned process and observation task; no other Host is addressed.
    pub(super) async fn close(&mut self) -> Result<(), AgentSessionError> {
        if self.closed {
            return Ok(());
        }
        let stopped = self.stop().await;
        // The Host has exited or been killed; its MCP credentials go with it.
        self.overlay = None;
        stopped
    }

    async fn stop(&mut self) -> Result<(), AgentSessionError> {
        self.diagnostics.flush();
        self.closed = true;
        self.reader.abort();
        #[cfg(unix)]
        process_group::signal(self.child.id(), Signal::Terminate);
        #[cfg(not(unix))]
        let _ = self.child.start_kill();
        if matches!(
            tokio::time::timeout(Duration::from_secs(5), self.child.wait()).await,
            Ok(Ok(_))
        ) {
            return Ok(());
        }
        process_group::signal(self.child.id(), Signal::Kill);
        let _ = self.child.start_kill();
        tokio::time::timeout(Duration::from_secs(2), self.child.wait())
            .await
            .map_err(|_| AgentSessionError::Failed)?
            .map_err(|_| AgentSessionError::Failed)?;
        Ok(())
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.reader.abort();
        process_group::signal(self.child.id(), Signal::Kill);
    }
}
