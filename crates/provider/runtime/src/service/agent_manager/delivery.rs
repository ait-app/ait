//! Serialized message admission, native interruption and durable retry receipts.

use std::time::Duration;

use super::{AgentManager, AgentManagerError};
use crate::protocol::agent_execution::ActiveTurnBehavior;
use crate::storage::timeline::Timeline;
use crate::storage::timeline::inbox::{QueuedInput, Receipt};
use crate::support::time::now_timestamp;
use model::ErrorCode;
use provider_core::{
    ports::agent_session::{AgentTurnEvent, CommandRoute},
    protocol::prompt::AgentPrompt,
};

/// Ceiling for writing a cancellation request while admitting newer input.
///
/// A wedged native process can leave cancellation writes waiting for the provider's RPC
/// deadline (60 s by default for OMP). Close the session when cancellation exceeds this
/// budget so queued input can be dispatched through a resumed session.
const INTERRUPT_WRITE_BUDGET: Duration = Duration::from_secs(10);

impl AgentManager {
    pub(crate) async fn deliver_answer(
        &mut self,
        agent: &str,
        prompt: &AgentPrompt,
    ) -> Result<(), ErrorCode> {
        self.require_managed(agent)
            .map_err(|_| ErrorCode::UnsupportedCapability)?;
        if let Some(receipt) = self.existing_input(agent, prompt, "answer")? {
            match receipt {
                Receipt::Accepted => return Ok(()),
                Receipt::Uncertain => return Err(ErrorCode::AgentIo),
                Receipt::Rejected | Receipt::New => {}
            }
        }
        if self.live.get(agent).is_some_and(|live| live.exclusive)
            || self.has_pending_input(agent)?
        {
            return Err(ErrorCode::CatalogBusy);
        }
        let timeline = self.timeline.clone().ok_or(ErrorCode::AgentIo)?;
        let message = prompt
            .client_message_id
            .as_deref()
            .ok_or(ErrorCode::InvalidMessage)?;
        match timeline.claim_answer(agent, message, prompt)? {
            Receipt::Accepted => return Ok(()),
            Receipt::Uncertain => return Err(ErrorCode::AgentIo),
            Receipt::Rejected => return Err(ErrorCode::InvalidMessage),
            Receipt::New => {}
        }
        match Box::pin(self.steer_input(agent, prompt)).await {
            Ok(()) => timeline.finish_input(agent, message, Receipt::Accepted),
            Err(
                AgentManagerError::Busy
                | AgentManagerError::InvalidRequest
                | AgentManagerError::PresetPending,
            ) => {
                timeline.finish_input(agent, message, Receipt::Rejected)?;
                Err(ErrorCode::CatalogBusy)
            }
            Err(_) => {
                timeline.finish_input(agent, message, Receipt::Uncertain)?;
                Err(ErrorCode::AgentIo)
            }
        }
    }

    pub(crate) fn claim_exclusive_turn(&mut self, agent: &str) -> Result<(), ErrorCode> {
        let live = self
            .live
            .get_mut(agent)
            .filter(|live| live.turn.is_some())
            .ok_or(ErrorCode::AgentIo)?;
        live.exclusive = true;
        Ok(())
    }

    pub(crate) fn has_pending_input(&self, agent: &str) -> Result<bool, ErrorCode> {
        if self
            .live
            .get(agent)
            .is_some_and(|live| live.session.pending_foreground())
        {
            return Ok(true);
        }
        self.timeline
            .as_ref()
            .map_or(Ok(false), |timeline| timeline.has_pending_admission(agent))
    }

    pub(crate) async fn deliver(
        &mut self,
        agent: &str,
        prompt: &AgentPrompt,
        behavior: ActiveTurnBehavior,
    ) -> Result<(), ErrorCode> {
        self.require_managed(agent)
            .map_err(|_| ErrorCode::UnsupportedCapability)?;
        prompt.validate().map_err(|_| ErrorCode::InvalidMessage)?;
        let record = self
            .registry
            .get(agent)
            .map_err(|_| ErrorCode::AgentIo)?
            .ok_or(ErrorCode::AgentNotFound)?;
        let behavior_policy = match behavior {
            ActiveTurnBehavior::Interrupt => "interrupt",
            ActiveTurnBehavior::Steer => "steer",
        };
        let Some(out_of_band) = self
            .command_input_route(agent, prompt, &record, behavior_policy)
            .await?
        else {
            return Ok(());
        };
        let policy = if out_of_band {
            "answer"
        } else {
            behavior_policy
        };
        if let Some(receipt) = self.existing_input(agent, prompt, policy)? {
            match receipt {
                Receipt::Accepted => return Ok(()),
                Receipt::Uncertain => return Err(ErrorCode::AgentIo),
                Receipt::Rejected if !out_of_band => return Err(ErrorCode::InvalidMessage),
                Receipt::Rejected | Receipt::New => {}
            }
        }
        if record.archived_at.is_some() || self.live.get(agent).is_some_and(|live| live.exclusive) {
            return Err(ErrorCode::CatalogBusy);
        }
        if out_of_band {
            return self.deliver_command(agent, prompt).await;
        }
        let timeline = self.timeline.clone().ok_or(ErrorCode::AgentIo)?;
        let message = prompt
            .client_message_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let queued = timeline.has_queued_input(agent)?;
        match timeline.reserve_input(agent, &message, prompt, policy)? {
            Receipt::Accepted => return Ok(()),
            Receipt::Uncertain => return Err(ErrorCode::AgentIo),
            Receipt::Rejected => return Err(ErrorCode::InvalidMessage),
            Receipt::New => {}
        }
        if self.active_turn(agent).is_some()
            && matches!(behavior, ActiveTurnBehavior::Steer)
            && !queued
        {
            timeline.claim_input(agent, &message)?;
            match Box::pin(self.steer_input(agent, prompt)).await {
                Ok(()) => return timeline.finish_input(agent, &message, Receipt::Accepted),
                Err(AgentManagerError::Busy) => timeline.requeue_input(agent, &message)?,
                Err(_) => {
                    timeline.finish_input(agent, &message, Receipt::Uncertain)?;
                    return Err(ErrorCode::AgentIo);
                }
            }
        }
        // A definitive rejection may mean the native turn just finished. Drain that result
        // before deciding whether interruption is still necessary.
        let _ = self.poll().await;
        if let Err(error) = self.interrupt_for_input(agent).await {
            timeline.finish_input(agent, &message, Receipt::Rejected)?;
            return Err(error);
        }
        // Native terminal acknowledgement may arrive later; the durable FIFO owns the input.
        Box::pin(self.dispatch_inputs(Some(agent))).await
    }

    /// Resolve routing from the current session; `None` means an input was already accepted.
    async fn command_input_route(
        &mut self,
        agent: &str,
        prompt: &AgentPrompt,
        record: &domain::agent_runtime::PersistedAgentRuntimeRecord,
        behavior_policy: &str,
    ) -> Result<Option<bool>, ErrorCode> {
        let client = self.clients.get(&record.provider);
        let static_candidate =
            client.is_some_and(|client| client.handles_out_of_band(&prompt.text));
        let candidate = static_candidate
            || (client.is_some_and(|client| client.commands_include_ownership())
                && prompt.text.trim_start().starts_with('/'));
        // A previously admitted input keeps its receipt even when the live command catalog
        // changes between attempts. Both checks still validate the complete prompt fingerprint.
        let mut prior_rejected = false;
        if candidate {
            let prior = match self.existing_input(agent, prompt, "answer") {
                Err(ErrorCode::IdempotencyConflict) => {
                    self.existing_input(agent, prompt, behavior_policy)?
                }
                result => result?,
            };
            match prior {
                Some(Receipt::Accepted) => return Ok(None),
                Some(Receipt::Uncertain) => return Err(ErrorCode::AgentIo),
                Some(Receipt::Rejected) => prior_rejected = true,
                Some(Receipt::New) | None => {}
            }
        }
        let out_of_band = if candidate {
            if record.archived_at.is_some()
                || self.live.get(agent).is_some_and(|live| live.exclusive)
            {
                return Err(ErrorCode::CatalogBusy);
            }
            if !self.live.contains_key(agent) {
                self.resume(agent).await.map_err(|_| ErrorCode::AgentIo)?;
            }
            let route = self
                .live
                .get(agent)
                .and_then(|live| live.session.command_route(&prompt.text));
            if prior_rejected && route.is_some() {
                return Err(ErrorCode::InvalidMessage);
            }
            match route {
                Some(CommandRoute::OutOfBand) => true,
                None => static_candidate,
                Some(CommandRoute::Foreground) => false,
                Some(CommandRoute::Pending) => return Err(ErrorCode::CatalogBusy),
                Some(CommandRoute::Unavailable) => return Err(ErrorCode::UnsupportedCapability),
            }
        } else {
            false
        };
        Ok(Some(out_of_band))
    }

    async fn deliver_command(
        &mut self,
        agent: &str,
        prompt: &AgentPrompt,
    ) -> Result<(), ErrorCode> {
        let mut prompt = prompt.clone();
        let message = prompt
            .client_message_id
            .get_or_insert_with(|| uuid::Uuid::new_v4().to_string())
            .clone();
        let timeline = self.timeline.clone().ok_or(ErrorCode::AgentIo)?;
        match timeline.claim_answer(agent, &message, &prompt)? {
            Receipt::Accepted => return Ok(()),
            Receipt::Uncertain => return Err(ErrorCode::AgentIo),
            Receipt::Rejected => return Err(ErrorCode::InvalidMessage),
            Receipt::New => {}
        }
        if self.resume(agent).await.is_err() {
            timeline.finish_input(agent, &message, Receipt::Rejected)?;
            return Err(ErrorCode::AgentIo);
        }
        self.execute_claimed_command(agent, &message, &prompt).await
    }

    /// Execute a command whose input receipt was claimed by immediate or creation admission.
    async fn execute_claimed_command(
        &mut self,
        agent: &str,
        message: &str,
        prompt: &AgentPrompt,
    ) -> Result<(), ErrorCode> {
        let timeline = self.timeline.clone().ok_or(ErrorCode::AgentIo)?;
        let config = self
            .registry
            .get(agent)
            .map_err(|_| ErrorCode::AgentIo)?
            .ok_or(ErrorCode::AgentNotFound)?
            .config
            .unwrap_or_default();
        let patch = self
            .live
            .get(agent)
            .ok_or(ErrorCode::AgentIo)?
            .session
            .command_config_patch(prompt, &config);
        let result = match patch {
            Ok(Some(patch)) => self
                .configure(agent, &patch)
                .await
                .map_err(|error| match error {
                    AgentManagerError::InvalidRequest
                    | AgentManagerError::SessionRejected
                    | AgentManagerError::Busy => {
                        provider_core::ports::agent_session::AgentSessionError::Rejected
                    }
                    _ => provider_core::ports::agent_session::AgentSessionError::Failed,
                }),
            Ok(None) => {
                self.live
                    .get_mut(agent)
                    .ok_or(ErrorCode::AgentIo)?
                    .session
                    .out_of_band(prompt)
                    .await
            }
            Err(error) => Err(error),
        };
        let receipt = match &result {
            Ok(()) => Receipt::Accepted,
            Err(provider_core::ports::agent_session::AgentSessionError::Rejected) => {
                Receipt::Rejected
            }
            Err(_) => Receipt::Uncertain,
        };
        timeline.finish_input(agent, message, receipt)?;
        if matches!(
            result,
            Err(provider_core::ports::agent_session::AgentSessionError::Rejected)
        ) {
            self.poll().await.map_err(|_| ErrorCode::AgentIo)?;
            return Err(ErrorCode::InvalidMessage);
        }
        result.map_err(|_| ErrorCode::AgentIo)?;
        self.poll().await.map_err(|_| ErrorCode::AgentIo)
    }

    fn existing_input(
        &self,
        agent: &str,
        prompt: &AgentPrompt,
        policy: &str,
    ) -> Result<Option<Receipt>, ErrorCode> {
        match (&self.timeline, &prompt.client_message_id) {
            (Some(timeline), Some(message)) => {
                timeline.input_receipt(agent, message, prompt, policy)
            }
            _ => Ok(None),
        }
    }

    /// Request cancellation before dispatching queued input, closing the session if the
    /// cancellation budget expires.
    async fn interrupt_for_input(&mut self, id: &str) -> Result<(), ErrorCode> {
        let Some(agent) = self.live.get_mut(id) else {
            return Ok(());
        };
        let write = async {
            agent
                .session
                .cancel_pending()
                .await
                .map_err(|_| ErrorCode::AgentIo)?;
            if let Some(turn) = &agent.turn
                && !agent.interruption_requested
            {
                if agent.session.cancel_turn(turn).await.is_err() {
                    agent.pending = Some(AgentTurnEvent::Failed);
                    return Err(ErrorCode::AgentIo);
                }
                agent.interruption_requested = true;
            }
            Ok(())
        };
        if let Ok(result) = tokio::time::timeout(INTERRUPT_WRITE_BUDGET, write).await {
            return result;
        }
        tracing::warn!(agent_id = %id, "Cancellation write timed out; closing native session");
        self.close(id).await.map_err(|_| ErrorCode::AgentIo)
    }

    /// Submit one FIFO head per idle Agent. Claim commits precede every native write.
    pub(crate) async fn dispatch_pending_inputs(&mut self) -> Result<(), ErrorCode> {
        self.dispatch_inputs(None).await
    }

    async fn dispatch_inputs(&mut self, selected: Option<&str>) -> Result<(), ErrorCode> {
        let Some(timeline) = self.timeline.clone() else {
            return Ok(());
        };
        let mut seen = std::collections::BTreeSet::new();
        for input in self.pending_inputs(&timeline, selected)? {
            if self
                .owner
                .as_ref()
                .is_some_and(|owner| !owner.owns(&input.agent))
                || selected.is_some_and(|agent| agent != input.agent)
            {
                continue;
            }
            if !seen.insert(input.agent.clone())
                || self.active_turn(&input.agent).is_some()
                || self
                    .live
                    .get(&input.agent)
                    .is_some_and(|live| live.session.pending_foreground())
            {
                continue;
            }
            let record = self
                .registry
                .get(&input.agent)
                .map_err(|_| ErrorCode::AgentIo)?;
            if record
                .as_ref()
                .is_none_or(|record| record.archived_at.is_some())
            {
                timeline.cancel_inputs(&input.agent)?;
                continue;
            }
            match self.ensure_preset_ready(&input.agent) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(_) => {
                    timeline.claim_input(&input.agent, &input.message)?;
                    self.fail_queued_input(&timeline, &input, &AgentManagerError::InvalidRequest)?;
                    if selected.is_some() {
                        return Err(ErrorCode::InvalidMessage);
                    }
                    continue;
                }
            }
            let provider = record
                .as_ref()
                .map_or("unknown", |entry| entry.provider.as_str());
            if !self
                .queued_command_ready(&timeline, &input, provider, selected)
                .await?
            {
                continue;
            }
            timeline.claim_input(&input.agent, &input.message)?;
            let admitted = Box::pin(self.admit_queued_input(&input)).await;
            match admitted {
                Ok(()) => timeline.finish_input(&input.agent, &input.message, Receipt::Accepted)?,
                Err(AgentManagerError::PresetPending) => {
                    timeline.requeue_input(&input.agent, &input.message)?;
                }
                Err(AgentManagerError::CommandCatalogChanged) => {
                    timeline.finish_input(&input.agent, &input.message, Receipt::Rejected)?;
                    if selected.is_some() {
                        return Err(ErrorCode::CatalogBusy);
                    }
                    Self::report_command_catalog_change(&timeline, &input, provider)?;
                }
                Err(error) => {
                    self.fail_queued_input(&timeline, &input, &error)?;
                    if selected.is_some() {
                        return Err(ErrorCode::AgentIo);
                    }
                }
            }
        }
        Ok(())
    }

    /// Recheck queued command ownership without consuming a pending catalog lookup.
    async fn queued_command_ready(
        &mut self,
        timeline: &Timeline,
        input: &QueuedInput,
        provider: &str,
        selected: Option<&str>,
    ) -> Result<bool, ErrorCode> {
        let ownership = self
            .clients
            .get(provider)
            .is_some_and(|client| client.commands_include_ownership());
        if (!ownership && !input.creation) || !input.prompt.text.trim_start().starts_with('/') {
            return Ok(true);
        }
        if !self.live.contains_key(&input.agent) {
            self.resume(&input.agent)
                .await
                .map_err(|_| ErrorCode::AgentIo)?;
        }
        self.poll().await.map_err(|_| ErrorCode::AgentIo)?;
        if self.active_turn(&input.agent).is_some() {
            return Ok(false);
        }
        let route = self
            .live
            .get(&input.agent)
            .and_then(|live| live.session.command_route(&input.prompt.text));
        if matches!(route, Some(CommandRoute::Pending)) {
            return Ok(false);
        }
        let out_of_band = route == Some(CommandRoute::OutOfBand)
            || route.is_none()
                && self
                    .clients
                    .get(provider)
                    .is_some_and(|client| client.handles_out_of_band(&input.prompt.text));
        if input.creation && out_of_band {
            timeline.claim_input(&input.agent, &input.message)?;
            // Only initial input is unclassified. A previously queued foreground command
            // must still be rejected if ownership changes while it waits (ADR-302).
            let result = self
                .execute_claimed_command(&input.agent, &input.message, &input.prompt)
                .await;
            if selected.is_some() {
                result?;
            }
            return Ok(false);
        }
        if matches!(
            route,
            Some(CommandRoute::OutOfBand | CommandRoute::Unavailable)
        ) {
            timeline.claim_input(&input.agent, &input.message)?;
            timeline.finish_input(&input.agent, &input.message, Receipt::Rejected)?;
            if selected.is_some() {
                return Err(ErrorCode::CatalogBusy);
            }
            Self::report_command_catalog_change(timeline, input, provider)?;
            return Ok(false);
        }
        Ok(true)
    }

    fn report_command_catalog_change(
        timeline: &Timeline,
        input: &QueuedInput,
        provider: &str,
    ) -> Result<(), ErrorCode> {
        timeline.append(&input.agent, provider, &[provider_core::protocol::timeline::NativeItem {
            key: format!("host:command-catalog-changed:{}", input.message),
            turn_id: None,
            timestamp: now_timestamp(),
            item: serde_json::json!({"type":"error","message":"The command list changed before this message could be sent. Please retry it."}),
        }])?;
        Ok(())
    }

    async fn admit_queued_input(&mut self, input: &QueuedInput) -> Result<(), AgentManagerError> {
        if input
            .message
            .starts_with(super::recovery::RESTART_INPUT_PREFIX)
        {
            self.resume_with_recovery(&input.agent, true).await?;
            self.poll().await?;
            if self
                .registry
                .get(&input.agent)
                .map_err(|_| AgentManagerError::Registry)?
                .is_some_and(|record| {
                    record.last_status == domain::agent_runtime::AgentRuntimeStatus::Error
                })
            {
                return Err(AgentManagerError::Session);
            }
            if self.active_turn(&input.agent).is_some()
                || self
                    .live
                    .get(&input.agent)
                    .is_some_and(|live| live.last_message.is_some())
            {
                return Ok(());
            }
        }
        Box::pin(self.send_input(&input.agent, &input.prompt)).await
    }

    fn fail_queued_input(
        &mut self,
        timeline: &Timeline,
        input: &QueuedInput,
        error: &AgentManagerError,
    ) -> Result<(), ErrorCode> {
        let state = if matches!(
            error,
            AgentManagerError::InvalidRequest
                | AgentManagerError::Busy
                | AgentManagerError::NotFound(_)
        ) {
            Receipt::Rejected
        } else {
            Receipt::Uncertain
        };
        timeline.finish_input(&input.agent, &input.message, state)?;
        let queued = timeline.has_queued_input(&input.agent)?;
        let previous = self
            .registry
            .get(&input.agent)
            .map_err(|_| ErrorCode::AgentIo)?;
        let reported = previous.as_ref().is_some_and(|record| {
            record.last_status == domain::agent_runtime::AgentRuntimeStatus::Error
                && record.last_error.is_some()
        });
        let message = previous
            .as_ref()
            .filter(|record| record.last_status == domain::agent_runtime::AgentRuntimeStatus::Error)
            .and_then(|record| record.last_error.clone())
            .unwrap_or_else(|| {
                if input
                    .message
                    .starts_with(super::recovery::RESTART_INPUT_PREFIX)
                {
                    format!("Execution could not resume after the host restarted: {error}")
                } else {
                    format!("Queued input could not be admitted: {error}")
                }
            });
        let now = now_timestamp();
        if !queued
            && previous
                .as_ref()
                .is_none_or(|record| record.last_error.as_deref() != Some(&message))
        {
            timeline.append(
                &input.agent,
                previous
                    .as_ref()
                    .map_or("unknown", |record| record.provider.as_str()),
                &[provider_core::protocol::timeline::NativeItem {
                    key: format!("host:provider-failure:input:{}", input.message),
                    turn_id: None,
                    timestamp: now.clone(),
                    item: serde_json::json!({"type":"error","message":message}),
                }],
            )?;
        }
        let committed = self
            .registry
            .update(&input.agent, &|current| {
                let mut next = current.clone();
                next.last_status = if queued {
                    domain::agent_runtime::AgentRuntimeStatus::Running
                } else {
                    domain::agent_runtime::AgentRuntimeStatus::Error
                };
                next.last_error = Some(message.clone());
                next.requires_attention = !queued;
                next.attention_reason =
                    (!queued).then_some(domain::agent_runtime::AgentAttentionReason::Error);
                next.attention_timestamp = (!queued).then(|| now.clone());
                next.updated_at.clone_from(&now);
                next
            })
            .map_err(|_| ErrorCode::AgentIo)?;
        if let Some(committed) = committed {
            if !queued && !committed.internal && !reported {
                self.events.publish(
                    domain::session::protocol::SessionEventKind::AgentAttention,
                    &serde_json::json!({"agentId":input.agent,"reason":"error","timestamp":now}),
                );
                timeline.events().publish(&input.agent,"agent_stream",&serde_json::json!({"agentId":input.agent,
                    "event":{"type":"turn_failed","provider":committed.provider,"error":message},"timestamp":now}));
            }
            if let Some(live) = self.live.get_mut(&input.agent) {
                live.record = committed;
            }
        }
        Ok(())
    }

    fn pending_inputs(
        &self,
        timeline: &Timeline,
        selected: Option<&str>,
    ) -> Result<Vec<QueuedInput>, ErrorCode> {
        if let Some(agent) = selected {
            return Ok(timeline.queued_for_agent(agent)?.into_iter().collect());
        }
        let Some(owner) = &self.owner else {
            return timeline.queued_inputs();
        };
        owner
            .agents()
            .iter()
            .filter(|agent| {
                self.active_turn(agent).is_none()
                    && !self
                        .live
                        .get(*agent)
                        .is_some_and(|live| live.session.pending_foreground())
            })
            .map(|agent| timeline.queued_for_agent(agent))
            .collect::<Result<Vec<_>, _>>()
            .map(|inputs| inputs.into_iter().flatten().collect())
    }
}
