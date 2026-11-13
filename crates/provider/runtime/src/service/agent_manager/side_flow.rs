//! BTW admission and settlement, independent of the main writer and input FIFO.

use crate::rpc::ErrorCode;

const SIDE_ADMISSION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
use crate::service::agent_manager::{AgentManager, AgentManagerError};
use crate::storage::side::{AskClaim, capture_snapshot, now_millis};
use provider_core::ports::agent_session::{AgentSessionError, AgentSessionSpec};
use provider_core::ports::side_questions::{SideAsk, SideCapability, SideRunnerSpec};
use provider_core::protocol::side_questions::{
    self, SideAskRequest, SideAskResponse, SideCancelRequest, SideCancelResponse,
    SideContextSnapshot, SideHistoryRequest, SideQuestionsState, SideReceiptState,
    SideTransferRequest, SideTransferResponse, SideTurnStatus, reason,
};
use sha2::Digest;

impl AgentManager {
    /// Release auxiliary resources without closing or mutating the foreground writer.
    pub(super) async fn close_side_runner(
        &mut self,
        agent_id: &str,
    ) -> Result<(), AgentManagerError> {
        if let Some(runner) = self.side_runners.get_mut(agent_id) {
            let closed = runner.close_side().await.map_err(super::map_session);
            let persisted = self
                .sync_side_topics(agent_id)
                .map_err(|_| AgentManagerError::Registry);
            let result = closed.and(persisted);
            if result.is_ok() {
                self.side_runners.remove(agent_id);
                self.side_dirty.remove(agent_id);
                self.side_specs.remove(agent_id);
                self.side_permits.remove(agent_id);
                self.side_activity.remove(agent_id);
            }
            result
        } else {
            Ok(())
        }
    }

    pub(super) async fn poll_side_runners(&mut self) -> Result<(), AgentManagerError> {
        for id in self.side_runners.keys().cloned().collect::<Vec<_>>() {
            if self.sync_side_topics(&id).is_err() {
                tracing::warn!(agent_id = %id, "Could not persist side-question progress");
            }
        }
        let idle_side = self
            .side_activity
            .iter()
            .filter(|(id, activity)| {
                activity.elapsed() >= std::time::Duration::from_secs(60)
                    && !self.side_permits.contains_key(*id)
            })
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in idle_side {
            self.close_side_runner(&id).await?;
        }
        Ok(())
    }

    /// Admit an idempotent ask using the frozen topic context, never the main input queue.
    /// # Errors
    /// Returns ownership, storage or malformed-request errors. Unknown acceptance is a receipt.
    pub(crate) async fn side_ask(
        &mut self,
        request: SideAskRequest,
    ) -> Result<SideAskResponse, ErrorCode> {
        self.require_managed(&request.agent_id)
            .map_err(|error| map_side_error(&error))?;
        if request.request_id.is_empty() || request.request_id.len() > 128 {
            return Err(ErrorCode::InvalidMessage);
        }
        let question = request.question.trim();
        if question.is_empty() || question.len() > side_questions::MAX_QUESTION_BYTES {
            return Ok(rejected_response(reason::INVALID_QUESTION));
        }
        self.sync_side_topics(&request.agent_id)?;
        let store = self.side_store.clone().ok_or(ErrorCode::AgentIo)?;
        if let Some(response) = claim_side_ask(&store, &request, question)? {
            return Ok(response);
        }
        let reject = |why: &str| reject_side_ask(&store, &request, why);
        if store.uncertain_active(&request.agent_id, &request.request_id)?
            || store
                .topics(&request.agent_id)?
                .iter()
                .any(|topic| topic.latest_turn().status == SideTurnStatus::Running)
        {
            return reject(reason::BUSY);
        }
        let record = self.control_record(&request.agent_id)?;
        if record.archived_at.is_some() || record.observation.is_some() {
            return reject(reason::UNSUPPORTED_PROVIDER);
        }
        let (snapshot, previous) = if let Some(topic_id) = request.record_id.as_deref() {
            let Some((topic, snapshot)) = store.topic(&request.agent_id, topic_id)? else {
                return reject(reason::UNKNOWN_TOPIC);
            };
            if topic.latest_turn().status != SideTurnStatus::Complete {
                return reject(reason::TOPIC_NOT_SETTLED);
            }
            (snapshot, Some(topic))
        } else {
            let timeline = self.timeline.as_ref().ok_or(ErrorCode::AgentIo)?;
            let snapshot = capture_snapshot(timeline, snapshot_identity(&record))?;
            (snapshot, None)
        };
        if snapshot.model.as_deref().is_none_or(str::is_empty) {
            return reject("no_selected_model");
        }
        match self.prepare_side_runner(&request.agent_id, &snapshot).await {
            Ok(()) => {}
            Err(ErrorCode::UnsupportedCapability) => return reject(reason::UNSUPPORTED_PROVIDER),
            Err(error) => {
                reject("runtime_inspection_failed")?;
                return Err(error);
            }
        }
        let runner = self
            .side_runners
            .get_mut(&request.agent_id)
            .ok_or(ErrorCode::AgentIo)?;
        if !runner.side_capability().available {
            return reject(reason::UNSUPPORTED_PROVIDER);
        }
        let strategy = runner.side_strategy();
        if previous
            .as_ref()
            .is_some_and(|topic| topic.strategy != strategy)
        {
            return reject(reason::UNSUPPORTED_PROVIDER);
        }
        let Ok(permit) = self.side_budget.clone().try_acquire_owned() else {
            return reject("capacity");
        };
        self.side_permits.insert(request.agent_id.clone(), permit);
        self.side_activity
            .insert(request.agent_id.clone(), std::time::Instant::now());
        store.prepare_ask(&request.agent_id, &request.request_id, strategy, &snapshot)?;
        let result = tokio::time::timeout(
            SIDE_ADMISSION_TIMEOUT,
            runner.side_ask(SideAsk {
                request_id: request.request_id.clone(),
                question: question.to_owned(),
                record_id: request.record_id.clone(),
                snapshot: snapshot.clone(),
                previous,
            }),
        )
        .await
        .unwrap_or(Err(AgentSessionError::Failed));
        self.settle_side_ask(&request, &snapshot, strategy, result)
    }

    fn settle_side_ask(
        &mut self,
        request: &SideAskRequest,
        snapshot: &SideContextSnapshot,
        strategy: side_questions::SideStrategy,
        result: Result<provider_core::ports::side_questions::SideAskReceipt, AgentSessionError>,
    ) -> Result<SideAskResponse, ErrorCode> {
        let store = self.side_store.clone().ok_or(ErrorCode::AgentIo)?;
        let reject = |why: &str| reject_side_ask(&store, request, why);
        match result {
            Ok(receipt) => {
                let topic = receipt.topic;
                let terminal = topic.latest_turn().status.is_terminal();
                let topic_id = topic.record.id.clone();
                if request.record_id.is_some() {
                    store.update_topic(&request.agent_id, topic)?;
                } else {
                    store.insert_topic(&request.agent_id, topic, snapshot)?;
                }
                let response = SideAskResponse {
                    receipt: SideReceiptState::Accepted,
                    record_id: Some(topic_id),
                    strategy: Some(receipt.strategy),
                    reason: None,
                };
                store.settle_ask(
                    &request.agent_id,
                    &request.request_id,
                    &response,
                    now_millis(),
                )?;
                if terminal {
                    store.release_uncertain(&request.agent_id, &request.request_id)?;
                }
                self.publish_runner_side(&request.agent_id);
                Ok(response)
            }
            Err(AgentSessionError::Busy) => {
                self.side_permits.remove(&request.agent_id);
                reject(reason::BUSY)
            }
            Err(AgentSessionError::Rejected) => {
                self.side_permits.remove(&request.agent_id);
                let reason = self
                    .side_runners
                    .get(&request.agent_id)
                    .and_then(|runner| runner.side_rejection_reason())
                    .unwrap_or(reason::INVALID_QUESTION)
                    .to_owned();
                self.publish_runner_side(&request.agent_id);
                reject(&reason)
            }
            Err(AgentSessionError::Unavailable) => {
                self.side_permits.remove(&request.agent_id);
                reject(reason::UNSUPPORTED_PROVIDER)
            }
            Err(_) => {
                let response = SideAskResponse {
                    receipt: SideReceiptState::Uncertain,
                    record_id: request.record_id.clone(),
                    strategy: Some(strategy),
                    reason: None,
                };
                store.settle_ask(
                    &request.agent_id,
                    &request.request_id,
                    &response,
                    now_millis(),
                )?;
                self.publish_runner_side(&request.agent_id);
                Ok(response)
            }
        }
    }

    pub(super) async fn prepare_side_runner(
        &mut self,
        agent: &str,
        snapshot: &SideContextSnapshot,
    ) -> Result<(), ErrorCode> {
        let record = self.control_record(agent)?;
        let mut config = record.config.clone().unwrap_or_default();
        config.model.clone_from(&snapshot.model);
        let session = self.confine_control(
            AgentSessionSpec {
                provider: record.provider.clone(),
                cwd: record.cwd.clone(),
                config,
                confinement: None,
            },
            record.workspace_id.as_deref(),
        )?;
        let directory = self
            .side_store
            .as_ref()
            .ok_or(ErrorCode::AgentIo)?
            .directory(agent)?;
        let environment = self
            .launch_environments
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(agent)
            .cloned()
            .unwrap_or_default();
        let mut digest = sha2::Sha256::new();
        for (key, value) in environment.entries() {
            digest.update(key.len().to_le_bytes());
            digest.update(key.as_bytes());
            digest.update(value.len().to_le_bytes());
            digest.update(value.as_bytes());
        }
        let selection = (session.clone(), <[u8; 32]>::from(digest.finalize()));
        if self.side_runners.contains_key(agent) {
            if self
                .side_specs
                .get(agent)
                .is_none_or(|spec| *spec == selection)
            {
                return Ok(());
            }
            let runner = self.side_runners.get_mut(agent).ok_or(ErrorCode::AgentIo)?;
            if self.side_permits.contains_key(agent)
                || self
                    .side_store
                    .as_ref()
                    .ok_or(ErrorCode::AgentIo)?
                    .uncertain_active(agent, "")?
            {
                return Err(ErrorCode::CatalogBusy);
            }
            runner.close_side().await.map_err(|_| ErrorCode::AgentIo)?;
            self.sync_side_topics(agent)?;
            self.side_runners.remove(agent);
            self.side_specs.remove(agent);
        }
        let client = self
            .clients
            .get(&record.provider)
            .ok_or(ErrorCode::UnsupportedCapability)?;
        let _probe = self
            .side_budget
            .clone()
            .try_acquire_owned()
            .map_err(|_| ErrorCode::ResourceExhausted)?;
        let runner = match tokio::time::timeout(
            SIDE_ADMISSION_TIMEOUT,
            client.create_side_questions(&SideRunnerSpec {
                session,
                directory,
                environment,
            }),
        )
        .await
        .map_err(|_| ErrorCode::AgentIo)?
        {
            Ok(runner) => runner,
            Err(AgentSessionError::Unavailable) => {
                self.publish_side_metadata(
                    agent,
                    SideQuestionsState::default(),
                    SideCapability::unavailable(
                        "This provider has no isolated BTW runtime on this host.",
                    ),
                );
                return Err(ErrorCode::UnsupportedCapability);
            }
            Err(_) => return Err(ErrorCode::AgentIo),
        };
        self.side_runners.insert(agent.to_owned(), runner);
        self.side_specs.insert(agent.to_owned(), selection);
        self.side_activity
            .insert(agent.to_owned(), std::time::Instant::now());
        self.publish_runner_side(agent);
        Ok(())
    }

    /// Cancel only the auxiliary runner; never report a failed cancellation as success.
    /// # Errors
    /// Returns ownership, storage or uncertain native cancellation errors.
    pub(crate) async fn side_cancel(
        &mut self,
        request: SideCancelRequest,
    ) -> Result<SideCancelResponse, ErrorCode> {
        self.require_managed(&request.agent_id)
            .map_err(|error| map_side_error(&error))?;
        self.reconcile_side_requests(&request.agent_id).await?;
        let Some(runner) = self.side_runners.get_mut(&request.agent_id) else {
            return Ok(SideCancelResponse { cancelled: false });
        };
        let outcome = runner
            .side_cancel(request.record_id.as_deref())
            .await
            .map_err(|_| ErrorCode::AgentIo)?;
        self.sync_side_topics(&request.agent_id)?;
        self.reconcile_side_requests(&request.agent_id).await?;
        if self
            .side_store
            .as_ref()
            .ok_or(ErrorCode::AgentIo)?
            .uncertain_active(&request.agent_id, "")?
        {
            return Err(ErrorCode::AgentIo);
        }
        Ok(SideCancelResponse {
            cancelled: outcome.cancelled,
        })
    }

    /// Read durable topics without starting a main writer or inference.
    /// # Errors
    /// Returns ownership or durable history failures.
    pub(crate) async fn side_history(
        &mut self,
        request: SideHistoryRequest,
    ) -> Result<SideQuestionsState, ErrorCode> {
        self.require_managed(&request.agent_id)
            .map_err(|error| map_side_error(&error))?;
        let store = self.side_store.clone().ok_or(ErrorCode::AgentIo)?;
        self.side_activity
            .insert(request.agent_id.clone(), std::time::Instant::now());
        let record = self.control_record(&request.agent_id)?;
        match if self.side_runners.contains_key(&request.agent_id) {
            Ok(())
        } else {
            self.prepare_side_runner(&request.agent_id, &snapshot_identity(&record))
                .await
        } {
            Ok(()) => {}
            Err(ErrorCode::UnsupportedCapability) => {
                let mut state = self.side_projection(&request.agent_id).unwrap_or_default();
                state.error = Some(
                    "Side-question runtime is unavailable; retained history is read-only."
                        .to_owned(),
                );
                return Ok(state);
            }
            Err(error) => return Err(error),
        }
        self.reconcile_side_requests(&request.agent_id).await?;
        if let Some(runner) = self.side_runners.get_mut(&request.agent_id) {
            let topics = runner
                .side_history()
                .await
                .map_err(|_| ErrorCode::AgentIo)?;
            for topic in topics {
                if store.topic(&request.agent_id, &topic.record.id)?.is_some() {
                    store.update_topic(&request.agent_id, topic)?;
                }
            }
        }
        Ok(self.side_projection(&request.agent_id).unwrap_or_default())
    }

    /// Return frozen context and Q/A for a new editable session without inference.
    /// # Errors
    /// Returns ownership, storage or unsettled-topic errors.
    pub(crate) fn side_transfer(
        &mut self,
        request: &SideTransferRequest,
    ) -> Result<SideTransferResponse, ErrorCode> {
        self.require_managed(&request.agent_id)
            .map_err(|error| map_side_error(&error))?;
        self.sync_side_topics(&request.agent_id)?;
        let store = self.side_store.as_ref().ok_or(ErrorCode::AgentIo)?;
        let (topic, snapshot) = store
            .topic(&request.agent_id, &request.record_id)?
            .ok_or(ErrorCode::InvalidMessage)?;
        if topic.latest_turn().status == SideTurnStatus::Running
            || store.uncertain_active(&request.agent_id, "")?
        {
            return Err(ErrorCode::CatalogBusy);
        }
        Ok(SideTransferResponse { snapshot, topic })
    }

    /// Persist observed side updates independently of foreground lifecycle.
    /// # Errors
    /// Returns storage failures without settling or cancelling the main turn.
    pub(crate) fn sync_side_topics(&mut self, agent: &str) -> Result<(), ErrorCode> {
        let Some(store) = self.side_store.as_ref() else {
            return Ok(());
        };
        let Some(runner) = self.side_runners.get_mut(agent) else {
            return Ok(());
        };
        let state = runner.poll_side().or_else(|| {
            self.side_dirty
                .contains(agent)
                .then(|| runner.side_projection())
        });
        let Some(mut state) = state else {
            return Ok(());
        };
        let capability = runner.side_capability();
        let persisted = std::mem::take(&mut state.records)
            .into_iter()
            .try_for_each(|topic| {
                if store.topic(agent, &topic.record.id)?.is_some() {
                    store.update_topic(agent, topic)?;
                }
                Ok::<_, ErrorCode>(())
            });
        if let Err(error) = persisted {
            self.side_dirty.insert(agent.to_owned());
            return Err(error);
        }
        self.side_dirty.remove(agent);
        if state.running_id.is_none() && !store.uncertain_active(agent, "")? {
            self.side_permits.remove(agent);
        }
        self.publish_side_metadata(agent, state, capability);
        Ok(())
    }

    fn publish_runner_side(&self, agent: &str) {
        if let Some(runner) = self.side_runners.get(agent) {
            self.publish_side_metadata(agent, runner.side_projection(), runner.side_capability());
        }
    }

    fn publish_side_metadata(
        &self,
        agent: &str,
        mut state: SideQuestionsState,
        capability: SideCapability,
    ) {
        state.available = Some(capability.available);
        state.unavailable_reason = capability.reason.map(|reason| {
            let mut safe = crate::storage::side::redact_text(&reason);
            safe.truncate(
                safe.floor_char_boundary(safe.len().min(side_questions::MAX_ERROR_BYTES)),
            );
            safe
        });
        if let Some(store) = &self.side_store {
            store.publish_live(agent, state);
        }
        if let Some(owner) = &self.owner {
            owner.side_changed();
        }
    }

    /// Overlay side state after primary runtime controls, without owning its native writer.
    pub(crate) fn side_control_snapshot(&self, agent: &str, snapshot: &mut serde_json::Value) {
        if let Some(state) = self.side_projection(agent) {
            if let Some(available) = state.available {
                snapshot["capabilities"]["sideQuestions"] = serde_json::json!(available);
            }
            snapshot["providerUi"]["sideQuestions"] = serde_json::json!(state);
        }
    }

    /// Return the bounded durable projection with observed live status only.
    pub(crate) fn side_projection(&self, agent: &str) -> Option<SideQuestionsState> {
        let store = self.side_store.as_ref()?;
        let topics = store.topics(agent).ok()?;
        let live_state = store.live(agent).or_else(|| {
            self.side_runners.get(agent).map(|runner| {
                let mut live = runner.side_projection();
                let capability = runner.side_capability();
                live.available = Some(capability.available);
                live.unavailable_reason = capability.reason;
                live
            })
        });
        if topics.is_empty() && live_state.is_none() {
            return None;
        }
        let omitted = store.topic_count(agent).ok()?.saturating_sub(topics.len());
        let mut state = SideQuestionsState {
            records: topics,
            omitted,
            ..Default::default()
        };
        if let Some(live) = live_state {
            state.available = live.available;
            state.unavailable_reason = live.unavailable_reason;
            state.running_id = live.running_id.filter(|id| {
                state.records.iter().any(|topic| {
                    topic.record.id == *id && topic.latest_turn().status == SideTurnStatus::Running
                })
            });
            state.reveal = live
                .reveal
                .filter(|id| state.records.iter().any(|topic| topic.record.id == *id));
            state.error = live.error.map(|error| {
                let mut safe = error;
                safe.truncate(
                    safe.floor_char_boundary(safe.len().min(side_questions::MAX_ERROR_BYTES)),
                );
                safe
            });
        }
        for topic in &mut state.records {
            if topic.latest_turn().status == SideTurnStatus::Running
                && state.running_id.as_deref() != Some(topic.record.id.as_str())
            {
                if let Some(turn) = topic.record.follow_ups.last_mut() {
                    turn.status = SideTurnStatus::Interrupted;
                } else {
                    topic.record.status = SideTurnStatus::Interrupted;
                }
            }
        }
        state.pending_acceptance = Some(store.uncertain_active(agent, "").ok()?);
        if state.pending_acceptance == Some(true) {
            state.error = Some(
                "Side-question acceptance is unknown; reconcile before asking again.".to_owned(),
            );
        }
        Some(state)
    }
}

fn snapshot_identity(
    record: &domain::agent_runtime::PersistedAgentRuntimeRecord,
) -> SideContextSnapshot {
    SideContextSnapshot {
        v: 1,
        agent_id: record.id.clone(),
        provider: record.provider.clone(),
        model: record
            .config
            .as_ref()
            .and_then(|config| config.model.clone())
            .or_else(|| {
                record
                    .runtime_info
                    .as_ref()
                    .and_then(|info| info.model.clone())
            }),
        native_session_id: record
            .runtime_info
            .as_ref()
            .and_then(|info| info.session_id.clone()),
        native_leaf_id: None,
        captured_at: now_millis(),
        entries: Vec::new(),
        truncated: false,
    }
}

fn rejected_response(why: &str) -> SideAskResponse {
    SideAskResponse {
        receipt: SideReceiptState::Rejected,
        record_id: None,
        strategy: None,
        reason: Some(why.to_owned()),
    }
}

fn map_side_error(error: &AgentManagerError) -> ErrorCode {
    match error {
        AgentManagerError::NotFound(_) => ErrorCode::AgentNotFound,
        AgentManagerError::InvalidRequest => ErrorCode::InvalidMessage,
        AgentManagerError::Busy => ErrorCode::CatalogBusy,
        _ => ErrorCode::AgentIo,
    }
}

fn claim_side_ask(
    store: &crate::storage::side::SideStore,
    request: &SideAskRequest,
    question: &str,
) -> Result<Option<SideAskResponse>, ErrorCode> {
    let claim = store.claim_ask(
        &request.agent_id,
        &request.request_id,
        question,
        request.record_id.as_deref(),
    )?;
    match claim {
        AskClaim::Accepted(record) | AskClaim::Rejected(record) | AskClaim::Uncertain(record) => {
            return Ok(Some(SideAskResponse {
                receipt: record.receipt,
                record_id: record.topic_id,
                strategy: record.strategy,
                reason: record.reason,
            }));
        }
        AskClaim::Conflict => return Err(ErrorCode::InvalidMessage),
        AskClaim::New => {}
    }
    Ok(None)
}

fn reject_side_ask(
    store: &crate::storage::side::SideStore,
    request: &SideAskRequest,
    why: &str,
) -> Result<SideAskResponse, ErrorCode> {
    let response = rejected_response(why);
    store.settle_ask(
        &request.agent_id,
        &request.request_id,
        &response,
        now_millis(),
    )?;
    Ok(response)
}
