//! `OpenCode` plugin implementing the same native session ports as Codex and Claude Code.
mod approvals;
mod bridge;
mod client;
mod live;
mod projection;
mod protocol;
mod publication;
mod runtime;
mod session;
mod summary;
mod types;

use std::path::PathBuf;

#[cfg(test)]
use protocol::history;
use protocol::http;
use types::{Fault, Invocation, Model, ProtocolError};
#[cfg(test)]
use {
    std::sync::Arc,
    types::{ProgressSink, Snapshot},
};

pub use client::OpenCodeClient;

/// Ceilings for content generated after the current input, excluding previous history.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[expect(
    clippy::struct_field_names,
    reason = "Each field names a ceiling, not an observed count"
)]
pub struct OpenCodeExecutionLimits {
    /// Maximum newly generated native content items.
    pub max_steps: u64,
    /// Maximum observed input, output and reasoning tokens.
    pub max_tokens: u64,
    /// Maximum newly generated text bytes, up to eight MiB.
    pub max_output_bytes: usize,
}

impl Default for OpenCodeExecutionLimits {
    fn default() -> Self {
        Self {
            max_steps: 128,
            max_tokens: 1_000_000,
            max_output_bytes: http::MAX_BODY,
        }
    }
}

#[derive(Clone, Debug)]
struct Driver {
    binary: PathBuf,
    limits: OpenCodeExecutionLimits,
}

impl Driver {
    fn new(binary: PathBuf) -> Self {
        Self {
            binary,
            limits: OpenCodeExecutionLimits::default(),
        }
    }

    #[cfg(test)]
    fn with_execution_limits(
        mut self,
        limits: OpenCodeExecutionLimits,
    ) -> Result<Self, ProtocolError> {
        if limits.max_steps == 0
            || limits.max_tokens == 0
            || limits.max_output_bytes == 0
            || limits.max_output_bytes > http::MAX_BODY
        {
            return Err(failure(
                Fault::AgentCapabilityUnsupported,
                "invalid OpenCode execution limits",
            ));
        }
        self.limits = limits;
        Ok(self)
    }

    async fn discover_models(&self, cwd: PathBuf) -> Result<Vec<Model>, ProtocolError> {
        let cancel = tokio_util::sync::CancellationToken::new();
        let mut runtime = runtime::Runtime::spawn(&self.binary, &cwd, &cancel).await?;
        let result = runtime.api.models().await;
        let _ = runtime.close().await;
        result
    }

    async fn open(&self, mut invocation: Invocation) -> Result<session::Connection, ProtocolError> {
        session::validate(&invocation)?;
        let mut runtime =
            runtime::Runtime::spawn(&self.binary, &invocation.cwd, &invocation.cancellation)
                .await?;
        match session::prepare(&runtime.api, &invocation).await {
            Ok(prepared) => {
                invocation.input_id.clone_from(&prepared.input_id);
                Ok(session::Connection {
                    runtime,
                    invocation,
                    prepared,
                    submitted: false,
                    limits: self.limits,
                })
            }
            Err(error) => {
                let _ = runtime.close().await;
                Err(error)
            }
        }
    }
}

fn failure(code: Fault, message: &'static str) -> ProtocolError {
    ProtocolError { code, message }
}

#[cfg(test)]
impl session::Connection {
    fn prepared(&self) -> &Snapshot {
        &self.prepared
    }
    async fn start(&mut self, progress: Arc<dyn ProgressSink>) -> Result<Snapshot, ProtocolError> {
        self.execute(progress).await
    }
    async fn read(&self) -> Result<Snapshot, ProtocolError> {
        session::snapshot(&self.runtime.api, &self.prepared.id, &self.invocation).await
    }
    async fn close(&mut self) {
        let _ = self.runtime.close().await;
    }
}

#[cfg(test)]
mod tests;
