//! Paseo's built-in slash commands (M5), run beside foreground input. Each request is written
//! without awaiting OMP's answer, which arrives with the session's events, so a long
//! compaction never blocks event polling or the manager lane.

use serde_json::{Value, json};

use super::super::commands::{self, AutoCompact, Builtin};
use super::{LABEL, Session, native_reason};
use provider::ports::agent_session::{AgentSessionError, AgentTurnEvent};
use provider::protocol::prompt::AgentPrompt;

/// Commands awaiting OMP's answer at once; more are refused before anything is written.
const MAX_PENDING: usize = 16;
/// Paseo's answer to an `/autocompact` argument it does not accept.
const AUTOCOMPACT_USAGE: &str = "[Error] Usage: /autocompact [on|off|toggle]";
/// Paseo's answer when `/autocompact toggle` cannot read OMP's current setting.
const AUTOCOMPACT_UNKNOWN: &str =
    "[Error] Auto-compaction state is unavailable. Use /autocompact on or /autocompact off.";

/// A built-in command awaiting OMP's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Pending {
    /// `/compact`, with the compaction id its rows share.
    Compact(String),
    /// `/autocompact toggle` reading OMP's current `autoCompactionEnabled`.
    AutoCompactState,
    /// `/autocompact` enabling (`true`) or disabling automatic compaction.
    AutoCompact(bool),
    /// `/handoff`.
    Handoff,
}

impl Session {
    /// Run one of Paseo's built-in slash commands in `prompt` (M5): `/steer` and `/follow-up`
    /// queue their message, or start an announced turn with it when idle; the others write
    /// their command and report OMP's answer when it arrives.
    ///
    /// # Errors
    ///
    /// Returns `AgentSessionError::Rejected` before writing for text that is no built-in,
    /// a closed session, a structured-output request, images or attachments on a command
    /// other than `/steer` and `/follow-up`, a second `/compact` while one runs, or too many
    /// unanswered commands. Returns write failures from the transport.
    pub(super) async fn run_command(
        &mut self,
        prompt: &AgentPrompt,
    ) -> Result<(), AgentSessionError> {
        prompt.validate()?;
        let (command, args) = commands::builtin(&prompt.text).ok_or(AgentSessionError::Rejected)?;
        if self.closed || prompt.output_schema.is_some() || self.pending.len() >= MAX_PENDING {
            return Err(AgentSessionError::Rejected);
        }
        let (kind, fields, pending) = match command {
            Builtin::Steer => return self.queue_message(prompt, args, "steer").await,
            Builtin::FollowUp => return self.queue_message(prompt, args, "followUp").await,
            _ if !prompt.images.is_empty() || !prompt.attachments.is_empty() => {
                return Err(AgentSessionError::Rejected);
            }
            // Paseo refuses a second compaction while one runs.
            Builtin::Compact if self.compacting() => return Err(AgentSessionError::Rejected),
            Builtin::Compact => ("compact", instructions(args), None),
            Builtin::Handoff => ("handoff", instructions(args), Some(Pending::Handoff)),
            Builtin::AutoCompact => match commands::auto_compact(args) {
                AutoCompact::Set(enabled) => (
                    "set_auto_compaction",
                    json!({"enabled":enabled}),
                    Some(Pending::AutoCompact(enabled)),
                ),
                AutoCompact::Toggle => ("get_state", json!({}), Some(Pending::AutoCompactState)),
                AutoCompact::Unknown => {
                    self.stream.command_note(AUTOCOMPACT_USAGE);
                    return Ok(());
                }
            },
        };
        // As in Paseo, a handled command ends any replay of the resumed session (M8).
        self.live = true;
        let id = self.transport.begin(kind, fields).await?;
        // The loading row shows once the request is written; OMP's own rows stay hidden.
        let pending =
            pending.unwrap_or_else(|| Pending::Compact(self.stream.begin_manual_compaction()));
        self.pending.insert(id, pending);
        Ok(())
    }

    /// Whether an Ait-issued `/compact` awaits OMP's answer.
    fn compacting(&self) -> bool {
        self.pending
            .values()
            .any(|pending| matches!(pending, Pending::Compact(_)))
    }

    /// Send `/steer` or `/follow-up`'s `message` with the prompt's images and attachments as
    /// a `prompt` with `behavior`: a queued input of the active turn, or when idle the start
    /// of a turn announced to the runtime, as Paseo does.
    async fn queue_message(
        &mut self,
        prompt: &AgentPrompt,
        message: &str,
        behavior: &str,
    ) -> Result<(), AgentSessionError> {
        let message = AgentPrompt {
            text: message.to_owned(),
            ..prompt.clone()
        };
        let payload = Self::payload(&message)?;
        let client = prompt.client_message_id.clone();
        let Some(turn) = self.active.clone() else {
            let turn = self.submit(payload, Some(behavior), client).await?;
            self.stream.events.push_back(AgentTurnEvent::Started(turn));
            return Ok(());
        };
        if self.input_requests.len() >= 128 {
            return Err(AgentSessionError::Rejected);
        }
        let id = self
            .transport
            .begin("prompt", payload.fields(Some(behavior)))
            .await?;
        // OMP queues it like a steer; a refusal is reported the same way (D6).
        self.input_requests.insert(id.clone(), turn);
        self.invalidate_state_check();
        self.stream.pending_inputs.push_back((id, client));
        Ok(())
    }

    /// Report OMP's answer to a pending built-in command, as Paseo reports it.
    pub(super) fn command_answered(&mut self, pending: Pending, event: &Value) {
        let success = event["success"] == true;
        match pending {
            Pending::Compact(id) => {
                self.stream
                    .end_manual_compaction(&id, success.then_some(&event["data"]));
                if success {
                    self.usage.refresh();
                } else {
                    self.command_failed("Failed to compact context", &event["error"]);
                }
            }
            Pending::AutoCompactState => {
                match event["data"]["autoCompactionEnabled"]
                    .as_bool()
                    .filter(|_| success)
                {
                    Some(enabled) => self.set_auto_compaction(!enabled),
                    None => self.stream.command_note(AUTOCOMPACT_UNKNOWN),
                }
            }
            Pending::AutoCompact(enabled) if success => {
                self.state["autoCompactionEnabled"] = json!(enabled);
                self.stream.command_note(if enabled {
                    "Auto-compaction enabled."
                } else {
                    "Auto-compaction disabled."
                });
            }
            Pending::AutoCompact(_) => {
                self.command_failed("Failed to set auto-compaction", &event["error"]);
            }
            // A handoff replaces the context with its summary.
            Pending::Handoff if success => self.usage.refresh(),
            Pending::Handoff => self.command_failed("Failed to hand off turn", &event["error"]),
        }
    }

    /// Continue `/autocompact toggle` with the opposite of OMP's setting, queued behind
    /// earlier writes because event polling cannot wait for a write.
    fn set_auto_compaction(&mut self, enabled: bool) {
        match self
            .transport
            .queue("set_auto_compaction", json!({"enabled":enabled}))
        {
            Ok(id) => {
                self.pending.insert(id, Pending::AutoCompact(enabled));
            }
            Err(_) => self.command_failed("Failed to set auto-compaction", &Value::Null),
        }
    }

    /// Show Paseo's `[Error] <action>: <reason>` note with OMP's bounded, redacted reason.
    fn command_failed(&mut self, action: &str, error: &Value) {
        let text = native_reason(error, "command").map_or_else(
            || format!("[Error] {action}."),
            |reason| {
                let reason = reason
                    .strip_prefix(LABEL)
                    .and_then(|rest| rest.strip_prefix(": "))
                    .unwrap_or(&reason);
                format!("[Error] {action}: {reason}")
            },
        );
        self.stream.command_note(&text);
    }
}

/// A command's `customInstructions`, omitted when blank.
fn instructions(args: &str) -> Value {
    if args.is_empty() {
        json!({})
    } else {
        json!({"customInstructions":args})
    }
}
