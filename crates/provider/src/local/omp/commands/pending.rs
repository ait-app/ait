//! Detached native query admission, independent of foreground prompt correlation.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::oneshot;

use crate::ports::agent_session::{AgentSessionError, AgentSessionFuture};
use crate::protocol::timeline::NativeItem;

type Sender = oneshot::Sender<Result<(), AgentSessionError>>;
type Receiver = oneshot::Receiver<Result<(), AgentSessionError>>;

#[derive(Debug, Default)]
pub(in super::super) struct Pending {
    text: Option<Text>,
    requests: BTreeMap<String, (String, Sender)>,
    completion: Option<(Receiver, Duration)>,
}

#[derive(Debug)]
struct Text {
    id: String,
    message: String,
    command: String,
    output: String,
    sender: Option<Sender>,
    deadline: Instant,
    malformed: bool,
}

/// Whether a frame belongs to a side control and must bypass foreground event projection.
pub(in super::super) enum Capture {
    Unrelated,
    Pending,
    Finished(NativeItem),
}

impl Pending {
    /// Check bounded pending admission before writing any native frame.
    pub(in super::super) fn check(&mut self, text: bool) -> Result<(), AgentSessionError> {
        self.requests.retain(|_, (_, sender)| !sender.is_closed());
        if self.completion.is_some() || self.requests.len() >= 32 || text && self.text.is_some() {
            return Err(AgentSessionError::Busy);
        }
        Ok(())
    }

    /// Track a native side-question RPC until its durable admission response.
    pub(in super::super) fn rpc(&mut self, id: String, method: &str, deadline: Duration) {
        let (sender, receiver) = oneshot::channel();
        self.requests.insert(id, (method.to_owned(), sender));
        self.completion = Some((receiver, deadline));
    }

    /// Track one query: native `command_output` frames have no ID, so queries must be serialized.
    pub(in super::super) fn query(
        &mut self,
        id: String,
        message: String,
        command: &str,
        deadline: Duration,
    ) {
        let (sender, receiver) = oneshot::channel();
        self.text = Some(Text {
            id,
            message,
            command: command.to_owned(),
            output: String::new(),
            sender: Some(sender),
            deadline: Instant::now() + deadline,
            malformed: false,
        });
        self.completion = Some((receiver, deadline));
    }

    /// Move final admission out of the Agent lane so streaming and approvals continue meanwhile.
    pub(in super::super) fn take(&mut self) -> Option<AgentSessionFuture<'static, ()>> {
        let (receiver, deadline) = self.completion.take()?;
        Some(Box::pin(async move {
            tokio::time::timeout(deadline, receiver)
                .await
                .map_err(|_| AgentSessionError::Failed)?
                .map_err(|_| AgentSessionError::Failed)?
        }))
    }

    /// Consume control responses and text without changing the foreground's last answer or ID.
    pub(in super::super) fn capture(&mut self, event: &Value) -> Capture {
        if event["type"] == "response"
            && let Some((method, sender)) =
                event["id"].as_str().and_then(|id| self.requests.remove(id))
        {
            let result = if event["command"] != method {
                Err(AgentSessionError::Failed)
            } else if event["success"] != true {
                Err(AgentSessionError::Rejected)
            } else {
                Ok(())
            };
            let _ = sender.send(result);
            return Capture::Pending;
        }
        let Some(pending) = self.text.as_mut() else {
            return Capture::Unrelated;
        };
        if event["type"] == "command_output" {
            let Some(text) = event["text"].as_str() else {
                pending.malformed = true;
                return Capture::Pending;
            };
            let room = (64 * 1024_usize).saturating_sub(pending.output.len());
            let mut end = room.min(text.len());
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            if !pending.output.is_empty() && room > 0 {
                pending.output.push('\n');
            }
            // Reserve the optional separator when clipping a final chunk.
            let mut end = end.min((64 * 1024_usize).saturating_sub(pending.output.len()));
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            pending.output.push_str(&text[..end]);
            return Capture::Pending;
        }
        if event["id"].as_str() != Some(pending.id.as_str()) {
            return Capture::Unrelated;
        }
        if event["type"] == "response" && event["success"] != true {
            let safe = super::super::bridge::redact_event(event);
            return self.finish(
                true,
                safe["error"].as_str().unwrap_or("Native query rejected."),
            );
        }
        if event["type"] == "response" && event["command"] == "prompt" {
            if event["data"]["agentInvoked"] == false {
                let malformed = pending.malformed;
                return self.finish(
                    malformed,
                    if malformed {
                        "OMP returned invalid command output."
                    } else {
                        ""
                    },
                );
            }
            if event["data"]["agentInvoked"] == true {
                return self.finish(true, "OMP unexpectedly invoked a model for a native query.");
            }
        }
        if event["type"] == "prompt_result" {
            let failed = pending.malformed
                || event["status"] != "completed"
                || event["agentInvoked"] == true;
            let reason = if pending.malformed {
                "OMP returned invalid command output."
            } else if failed {
                "OMP did not complete the native query without model work."
            } else {
                ""
            };
            return self.finish(failed, reason);
        }
        Capture::Pending
    }

    fn finish(&mut self, failed: bool, error: &str) -> Capture {
        let pending = self
            .text
            .take()
            .expect("finishing an owned native text control");
        if let Some(sender) = pending.sender {
            let _ = sender.send(if failed {
                Err(AgentSessionError::Rejected)
            } else {
                Ok(())
            });
        }
        let output = if failed {
            error
        } else if pending.output.is_empty() {
            "Native query completed."
        } else {
            &pending.output
        };
        Capture::Finished(super::note(
            &pending.message,
            &pending.command,
            failed,
            output,
        ))
    }

    /// Report expiry once, retaining correlation until the native handler actually settles.
    pub(in super::super) fn expired(&mut self) -> Option<NativeItem> {
        let pending = self.text.as_mut()?;
        if Instant::now() < pending.deadline {
            return None;
        }
        let sender = pending.sender.take()?;
        let _ = sender.send(Err(AgentSessionError::Failed));
        Some(super::note(
            &pending.message,
            &pending.command,
            true,
            "The native query timed out. The main conversation remains available; OMP may still be finishing this query.",
        ))
    }

    /// A small status owns the query's waiting domain, leaving the editor and foreground free.
    pub(in super::super) fn status(&self) -> Option<String> {
        self.text.as_ref().map(|pending| {
            if pending.sender.is_some() {
                format!("Running /{}", pending.command)
            } else {
                format!(
                    "Waiting for OMP to settle /{} after timeout",
                    pending.command
                )
            }
        })
    }

    /// Release detached receipts when their native owner closes.
    pub(in super::super) fn close(&mut self) {
        self.text = None;
        self.requests.clear();
        self.completion = None;
    }
}

#[cfg(test)]
mod tests;
