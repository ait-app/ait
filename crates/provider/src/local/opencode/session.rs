//! Prepare, submit once, observe and reconcile; cancellation never becomes an input retry.
use std::{sync::Arc, time::Duration};

use crate::local::opencode::types::{Fault, ProtocolError};
use crate::local::opencode::types::{Invocation, ProgressSink, Snapshot};

use super::{
    approvals::{Pending, Resolution},
    failure,
    protocol::http::{Api, Events},
    runtime::Runtime,
};

pub(super) struct Connection {
    pub(super) runtime: Runtime,
    pub(super) invocation: Invocation,
    pub(super) prepared: Snapshot,
    pub(super) submitted: bool,
    pub(super) limits: super::OpenCodeExecutionLimits,
}

enum Observation {
    Settled(Snapshot),
    PermissionDeclined,
}

pub(super) enum Submission {
    Observing(Events),
    Settled(Snapshot),
}

pub(super) fn validate(request: &Invocation) -> Result<(), ProtocolError> {
    if request.driver != "opencode"
        || !request.cwd.is_absolute()
        || request.input_id.is_empty()
        || request.request_id.is_empty()
        || request
            .instructions
            .as_ref()
            .is_some_and(|text| text.len() > 1024 * 1024)
        || request.prompt.len() > 1024 * 1024
        || request.model.split_once('/').is_none()
        || request.session_id.as_ref().is_some_and(|id| !valid_id(id))
    {
        return Err(failure(
            Fault::AgentCapabilityUnsupported,
            "invalid OpenCode session invocation",
        ));
    }
    if !request.full_access {
        return Err(failure(
            Fault::AgentCapabilityUnsupported,
            "OpenCode native permissions are not an OS sandbox; this adapter requires explicit full access",
        ));
    }
    if request.session_id.is_some() && request.instructions.is_some() {
        return Err(failure(
            Fault::AgentCapabilityUnsupported,
            "OpenCode resume cannot replace session instructions",
        ));
    }
    if request.cancellation.is_cancelled() {
        return Err(failure(Fault::RunCancelled, "OpenCode admission cancelled"));
    }
    Ok(())
}

pub(super) fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 512
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub(super) async fn prepare(api: &Api, request: &Invocation) -> Result<Snapshot, ProtocolError> {
    let models = api.models().await?;
    if !models.iter().any(|model| {
        model.id == request.model
            && request
                .reasoning_effort
                .as_ref()
                .is_none_or(|effort| model.reasoning_efforts.contains(effort))
    }) {
        return Err(failure(
            Fault::AgentCapabilityUnsupported,
            "OpenCode model or variant is unavailable",
        ));
    }
    api.prepare_session(request).await
}

pub(super) async fn snapshot(
    api: &Api,
    id: &str,
    request: &Invocation,
) -> Result<Snapshot, ProtocolError> {
    api.snapshot(id, request).await
}

#[cfg(test)]
mod tests;

impl Connection {
    #[cfg(test)]
    pub(super) async fn execute(
        &mut self,
        progress: Arc<dyn ProgressSink>,
    ) -> Result<Snapshot, ProtocolError> {
        match self.submit().await? {
            Submission::Settled(snapshot) => Ok(snapshot),
            Submission::Observing(events) => self.observe(events, progress).await,
        }
    }

    pub(super) async fn submit(&mut self) -> Result<Submission, ProtocolError> {
        if self.submitted {
            return Err(failure(
                Fault::RunRecoveryFailed,
                "OpenCode input must not be replayed",
            ));
        }
        if self.invocation.cancellation.is_cancelled() {
            return Err(failure(
                Fault::RunCancelled,
                "OpenCode input cancelled before submission",
            ));
        }
        let events = self.ready_events().await?;
        self.invocation.input_id.clone_from(&self.prepared.input_id);
        // Mark before polling the future: even a lost admission response may have produced effects.
        self.submitted = true;
        let submission = self
            .runtime
            .api
            .submit_input(&self.prepared.id, &self.invocation)
            .await;
        if submission.is_err() {
            // Reconcile once; never dispatch again after transport ambiguity.
            if let Ok(history) =
                snapshot(&self.runtime.api, &self.prepared.id, &self.invocation).await
                && accepted(&history)
            {
                self.check_budget().await?;
                return Ok(Submission::Settled(history));
            }
            return Err(failure(
                Fault::RunRecoveryFailed,
                "OpenCode input outcome unknown; reconcile history without replay",
            ));
        }
        Ok(Submission::Observing(events))
    }

    async fn interrupt(&self) {
        let _ = self.runtime.api.interrupt(&self.prepared.id).await;
    }

    async fn check_budget(&self) -> Result<(), ProtocolError> {
        let raw = self.runtime.api.history(&self.prepared.id).await?;
        if let Err(error) = self
            .runtime
            .api
            .validate_budget(&raw, &self.prepared, self.limits)
        {
            self.interrupt().await;
            return Err(error);
        }
        Ok(())
    }

    async fn ready_events(&self) -> Result<Events, ProtocolError> {
        tokio::select! {
            () = self.invocation.cancellation.cancelled() => Err(failure(Fault::RunCancelled, "OpenCode observation cancelled")),
            result = tokio::time::timeout(Duration::from_secs(30), async {
                let mut events = self.runtime.api.events().await?;
                events.wait_ready().await?;
                Ok(events)
            }) => result.unwrap_or_else(|_| Err(failure(Fault::RunRecoveryFailed, "OpenCode event stream was not ready; input was not sent"))),
        }
    }

    pub(super) async fn observe(
        &self,
        events: Events,
        progress: Arc<dyn ProgressSink>,
    ) -> Result<Snapshot, ProtocolError> {
        let result = tokio::select! {
            () = self.invocation.cancellation.cancelled() =>
                Err(failure(Fault::RunCancelled, "OpenCode observation cancelled")),
            result = self.observe_inner(events, progress) => result,
        };
        match result {
            Ok(Observation::Settled(history)) => Ok(history),
            Ok(Observation::PermissionDeclined) => {
                // Reject may already have ended the native turn. Do not interrupt an idle
                // session; the caller still reconciles current-input history before release.
                if !self
                    .invocation
                    .cancel_acknowledged
                    .load(std::sync::atomic::Ordering::Acquire)
                    && !self.runtime.api.idle(&self.prepared.id).await?
                {
                    self.runtime.api.interrupt(&self.prepared.id).await?;
                }
                Err(failure(Fault::RunCancelled, "OpenCode permission declined"))
            }
            Err(error) => {
                if error.code == Fault::RunCancelled
                    && !self
                        .invocation
                        .cancel_acknowledged
                        .load(std::sync::atomic::Ordering::Acquire)
                {
                    self.runtime.api.interrupt(&self.prepared.id).await?;
                }
                Err(error)
            }
        }
    }

    async fn observe_inner(
        &self,
        mut events: Events,
        progress: Arc<dyn ProgressSink>,
    ) -> Result<Observation, ProtocolError> {
        let mut timer = tokio::time::interval(Duration::from_secs(5));
        let mut displayed =
            self.runtime
                .api
                .stream(&self.prepared.id, &self.prepared.input_id, self.limits);
        let mut approvals = Pending::new();
        let mut publication = super::publication::Publication::default();
        loop {
            tokio::select! {
                event = events.next() => {
                    if let Err(error) = &event && error.code == Fault::RunLimitExceeded {
                        self.interrupt().await;
                        return Err(*error);
                    }
                    if let Ok(Some(event)) = event {
                        if let Some(data) = Api::permission_event(&event, &self.prepared.id) {
                            approvals.observe(&self.runtime.api,&self.invocation,&self.prepared.id,data)?;
                        }
                        let updates = match displayed.observe(&event) {
                            Ok(updates) => updates,
                            Err(error) => {
                                if error.code == Fault::RunLimitExceeded {self.interrupt().await;}
                                return Err(error);
                            }
                        };
                        for update in updates {
                            if let Err(error) = publication.report(self, &progress, update).await {
                                if error.code == Fault::RunLimitExceeded { self.interrupt().await; }
                                return Err(error);
                            }
                        }
                    } else {
                        // Lost events require state reconciliation; they never imply execution failure.
                        if let Ok(history) = snapshot(&self.runtime.api, &self.prepared.id, &self.invocation).await
                            && accepted(&history) && approvals.can_settle() { self.check_budget().await?; return Ok(Observation::Settled(history)); }
                        events = self.ready_events().await?;
                    }
                }
                _ = timer.tick() => {
                    let raw = self.runtime.api.history(&self.prepared.id).await?;
                    if let Err(error) = self.runtime.api.validate_budget(&raw, &self.prepared, self.limits) {
                        self.interrupt().await;
                        return Err(error);
                    }
                    approvals.reconcile(&self.runtime.api,&self.invocation,&self.prepared.id).await?;
                    if let Ok(history) = snapshot(&self.runtime.api, &self.prepared.id, &self.invocation).await
                        && accepted(&history) && approvals.can_settle() {
                        return Ok(Observation::Settled(history));
                    }
                }
                result = approvals.receiver.recv() => {
                    if let Some(result) = result
                        && result? == Resolution::Declined
                    {
                        return Ok(Observation::PermissionDeclined);
                    }
                }
            }
        }
    }
}

pub(super) fn accepted(snapshot: &Snapshot) -> bool {
    let Some(index) = snapshot
        .messages
        .iter()
        .position(|message| message.input_id.as_ref() == Some(&snapshot.input_id))
    else {
        return false;
    };
    snapshot.outcome.is_some_and(|outcome| {
        outcome != crate::local::opencode::types::Outcome::Completed
            || snapshot.messages[index + 1..]
                .iter()
                .any(|message| message.role == crate::local::opencode::types::Role::Assistant)
    })
}
