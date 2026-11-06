//! Admission and refusal of commands that must not replace a foreground model turn.

use serde_json::json;

use super::super::commands;
use super::Session;
use crate::ports::agent_session::{AgentSessionError, AgentTurnEvent};
use crate::protocol::prompt::AgentPrompt;

impl Session {
    /// Render a definitive rejection without sending a terminal-only command to inference.
    pub(super) fn reject_command(&mut self, prompt: &AgentPrompt, name: &str) {
        let id = prompt
            .client_message_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        self.stream
            .events
            .push_back(AgentTurnEvent::Timeline(commands::note(
                &id,
                name,
                true,
                &commands::rejection(name),
            )));
    }

    /// Write only native control input; final admission is detached from this Agent's lane.
    pub(super) async fn side_command(
        &mut self,
        prompt: &AgentPrompt,
    ) -> Result<(), AgentSessionError> {
        prompt.validate()?;
        if self.closed
            || self.actions.changing_history()
            || !prompt.images.is_empty()
            || !prompt.attachments.is_empty()
            || prompt.output_schema.is_some()
        {
            return Err(AgentSessionError::Rejected);
        }
        if let Some(name) = commands::blocked(&prompt.text, &self.native_commands) {
            self.reject_command(prompt, name);
            return Ok(());
        }
        let (name, args) = commands::split(&prompt.text).ok_or(AgentSessionError::Rejected)?;
        if name == "btw" {
            if args.len() > 65536 {
                return Err(AgentSessionError::Rejected);
            }
            self.command_controls.check(false)?;
            let (method, fields) = if args.is_empty() {
                ("get_btw_history", json!({}))
            } else {
                ("btw", json!({"question":args}))
            };
            let id = self.transport.begin(method, fields).await?;
            self.command_controls.rpc(id, method, self.client.deadline);
            return Ok(());
        }
        if !commands::read_only(&prompt.text, &self.native_commands) {
            return Err(AgentSessionError::Rejected);
        }
        if self.active_builtin {
            return Err(AgentSessionError::Busy);
        }
        self.command_controls.check(true)?;
        let id = self
            .transport
            .begin("prompt", json!({"message":prompt.text,"images":[]}))
            .await?;
        let message = prompt
            .client_message_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        self.command_controls
            .query(id, message, name, self.client.deadline);
        Ok(())
    }
}
