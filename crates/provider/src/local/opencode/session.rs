//! Prepare, submit once, observe and reconcile; cancellation never becomes an input retry.
use std::{sync::Arc, time::Duration};

use crate::local::opencode::types::{Fault, ProtocolError};
use crate::local::opencode::types::{Invocation, ProgressSink, Snapshot};
use reqwest::Method;
use serde_json::{Value, json};

use super::{
    approvals::Pending,
    failure, history,
    http::{Api, Events, Version, required_string},
    runtime::Runtime,
    streaming::Stream,
};

pub(super) struct Connection {
    pub(super) runtime: Runtime,
    pub(super) invocation: Invocation,
    pub(super) prepared: Snapshot,
    pub(super) submitted: bool,
    pub(super) limits: super::OpenCodeExecutionLimits,
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
    let permission = permissions(api.version);
    let id = if let Some(id) = &request.session_id {
        if !api.idle(id).await? {
            return Err(failure(
                Fault::SessionBusy,
                "OpenCode session is active elsewhere",
            ));
        }
        if api.version == Version::V2 {
            api.json(
                Method::PATCH,
                &api.path(id, ""),
                Some(&json!({"permissions":permission})),
            )
            .await?;
            api.json(
                Method::POST,
                &api.path(id, "/model"),
                Some(&json!({"model":model(request, api.version)})),
            )
            .await?;
        }
        // V1 PATCH appends rules. Reuse the creation policy and verify it in snapshot.
        id.clone()
    } else {
        let body = match api.version {
            Version::V1 => json!({"permission":permission}),
            Version::V2 => json!({"location":{"directory":request.cwd}, "agent":"build",
                "model":model(request, api.version), "permissions":permission}),
        };
        let response = api
            .json(
                Method::POST,
                &format!("{}/session", api.version.prefix()),
                Some(&body),
            )
            .await?;
        let id = required_string(api.data(&response), "id")?;
        if !valid_id(id) {
            return Err(failure(
                Fault::ProviderFailed,
                "invalid OpenCode session identity",
            ));
        }
        if api.version == Version::V2
            && let Some(instructions) = &request.instructions
        {
            let path = format!("/api/experimental/session/{id}/instructions/entries/ait");
            api.json(Method::PUT, &path, Some(&json!({"value":instructions})))
                .await?;
        }
        id.to_owned()
    };
    let mut history = snapshot(api, &id, request).await?;
    if api.version == Version::V1 && !history.input_id.starts_with("msg_") {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let random = uuid::Uuid::new_v4().simple().to_string();
        history.input_id = format!(
            "msg_{:012x}{}",
            (timestamp << 12) & 0xffff_ffff_ffff,
            &random[..14]
        );
    }
    Ok(history)
}

fn model(request: &Invocation, version: Version) -> Value {
    let (provider, model) = request
        .model
        .split_once('/')
        .expect("validated provider/model identifier");
    let mut selected = match version {
        Version::V1 => json!({"providerID":provider,"modelID":model}),
        Version::V2 => json!({"providerID":provider,"id":model}),
    };
    if version == Version::V2
        && let Some(variant) = &request.reasoning_effort
    {
        selected["variant"] = json!(variant);
    }
    selected
}

fn permissions(version: Version) -> Value {
    let rules = [
        ("*", "deny"),
        ("read", "allow"),
        ("glob", "allow"),
        ("grep", "allow"),
        ("list", "allow"),
        ("skill", "allow"),
        ("todowrite", "allow"),
        ("edit", "ask"),
        (
            if version == Version::V1 {
                "bash"
            } else {
                "shell"
            },
            "ask",
        ),
    ];
    Value::Array(
        rules
            .into_iter()
            .map(|(name, action)| match version {
                Version::V1 => json!({"permission":name,"pattern":"*","action":action}),
                Version::V2 => json!({"action":name,"resource":"*","effect":action}),
            })
            .collect(),
    )
}

fn permissions_match(version: Version, actual: Option<&Value>) -> bool {
    let expected = permissions(version);
    if version == Version::V2 {
        return actual == Some(&expected);
    }
    let Some(rules) = actual.and_then(Value::as_array) else {
        return false;
    };
    let expected = expected.as_array().expect("permission policy is an array");
    // Older Ait resumes appended whole copies of this policy. Only accept exact copies,
    // preserving rule order and rejecting any additional grants or partial policies.
    !rules.is_empty() && rules.chunks(expected.len()).all(|chunk| chunk == expected)
}

pub(super) async fn snapshot(
    api: &Api,
    id: &str,
    request: &Invocation,
) -> Result<Snapshot, ProtocolError> {
    if !api.idle(id).await? {
        return Err(failure(
            Fault::SessionBusy,
            "OpenCode execution is not idle",
        ));
    }
    let response = api.json(Method::GET, &api.path(id, ""), None).await?;
    let info = api.data(&response);
    let cwd = match api.version {
        Version::V1 => info.get("directory"),
        Version::V2 => info.pointer("/location/directory"),
    }
    .and_then(Value::as_str)
    .ok_or_else(|| failure(Fault::ProviderFailed, "OpenCode cwd missing"))?;
    let effective_permissions = match api.version {
        Version::V1 => info.get("permission"),
        Version::V2 => info.get("permissions"),
    };
    if required_string(info, "id")? != id
        || std::path::Path::new(cwd) != request.cwd
        || (request.verify_settings && !permissions_match(api.version, effective_permissions))
    {
        return Err(failure(
            Fault::AgentCapabilityUnsupported,
            "OpenCode session identity, cwd or permissions differ from admission",
        ));
    }
    if request.verify_settings
        && api.version == Version::V2
        && (info.pointer("/model/providerID").and_then(Value::as_str)
            != request.model.split_once('/').map(|(provider, _)| provider)
            || info.pointer("/model/id").and_then(Value::as_str)
                != request.model.split_once('/').map(|(_, model)| model)
            || request.reasoning_effort.as_deref().is_some_and(|effort| {
                info.pointer("/model/variant").and_then(Value::as_str) != Some(effort)
            }))
    {
        return Err(failure(
            Fault::AgentCapabilityUnsupported,
            "OpenCode effective model differs from admission",
        ));
    }
    let execution = if api.version == Version::V2 {
        api.execution(id).await?
    } else {
        None
    };
    let raw = api.history(id).await?;
    let outcome = super::execution::outcome(api.version, info, execution.as_ref(), &raw)?;
    if api.version == Version::V2 && api.execution(id).await? != execution {
        return Err(failure(
            Fault::SessionBusy,
            "OpenCode execution changed during reconciliation",
        ));
    }
    if api.version == Version::V2 && execution.is_some() && outcome.is_none() {
        return Err(failure(
            Fault::SessionBusy,
            "OpenCode durable execution has not drained",
        ));
    }
    let messages = history::normalize(api.version, id, &raw)?;
    if !api.idle(id).await? {
        return Err(failure(
            Fault::SessionBusy,
            "OpenCode became active during history reconciliation",
        ));
    }
    Ok(Snapshot {
        driver: "opencode".into(),
        id: id.into(),
        input_id: request.input_id.clone(),
        cwd: request.cwd.clone(),
        model: request.model.clone(),
        reasoning_effort: request.reasoning_effort.clone(),
        messages,
        outcome,
    })
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
        let mut body = match self.runtime.api.version {
            Version::V1 => {
                json!({"messageID":self.invocation.input_id,"parts":[{"type":"text","text":self.invocation.prompt}],
                "model":model(&self.invocation, Version::V1)})
            }
            Version::V2 => json!({"text":self.invocation.prompt,"files":[],
                "metadata":{"aitInputId":self.invocation.input_id}}),
        };
        if self.runtime.api.version == Version::V1 {
            if let Some(variant) = &self.invocation.reasoning_effort {
                body["variant"] = json!(variant);
            }
            if let Some(instructions) = &self.invocation.instructions {
                body["system"] = json!(instructions);
            }
        }
        let suffix = if self.runtime.api.version == Version::V1 {
            "/prompt_async"
        } else {
            "/prompt"
        };
        // Mark before polling the future: even a lost admission response may have produced effects.
        self.submitted = true;
        let submission = self
            .runtime
            .api
            .json(
                Method::POST,
                &self.runtime.api.path(&self.prepared.id, suffix),
                Some(&body),
            )
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
        let suffix = if self.runtime.api.version == Version::V1 {
            "/abort"
        } else {
            "/interrupt"
        };
        let _ = tokio::time::timeout(
            Duration::from_secs(5),
            self.runtime.api.json(
                Method::POST,
                &self.runtime.api.path(&self.prepared.id, suffix),
                Some(&json!({})),
            ),
        )
        .await;
    }

    async fn check_budget(&self) -> Result<(), ProtocolError> {
        let raw = self.runtime.api.history(&self.prepared.id).await?;
        if let Err(error) =
            super::budget::validate(self.runtime.api.version, &raw, &self.prepared, self.limits)
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
                connected(&mut events).await?;
                Ok(events)
            }) => result.unwrap_or_else(|_| Err(failure(Fault::RunRecoveryFailed, "OpenCode event stream was not ready; input was not sent"))),
        }
    }

    pub(super) async fn observe(
        &self,
        events: Events,
        progress: Arc<dyn ProgressSink>,
    ) -> Result<Snapshot, ProtocolError> {
        let cancel = self.invocation.cancellation.clone();
        let result = tokio::select! {
            () = cancel.cancelled() => Err(failure(Fault::RunCancelled, "OpenCode observation cancelled")),
            result = self.observe_inner(events, progress) => result,
        };
        if result
            .as_ref()
            .is_err_and(|error| error.code == Fault::RunCancelled)
            && !self
                .invocation
                .cancel_acknowledged
                .load(std::sync::atomic::Ordering::Acquire)
        {
            let suffix = if self.runtime.api.version == Version::V1 {
                "/abort"
            } else {
                "/interrupt"
            };
            tokio::time::timeout(
                Duration::from_secs(5),
                self.runtime.api.json(
                    Method::POST,
                    &self.runtime.api.path(&self.prepared.id, suffix),
                    Some(&json!({})),
                ),
            )
            .await
            .map_err(|_| failure(Fault::RunRecoveryFailed, "interrupt timed out"))??;
        }
        result
    }

    async fn observe_inner(
        &self,
        mut events: Events,
        progress: Arc<dyn ProgressSink>,
    ) -> Result<Snapshot, ProtocolError> {
        let mut timer = tokio::time::interval(Duration::from_secs(5));
        let mut displayed = Stream::new(&self.prepared.id, &self.prepared.input_id, self.limits);
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
                        let event = event.get("payload").unwrap_or(&event);
                        let data = event.get("properties").or_else(||event.get("data")).unwrap_or(&Value::Null);
                        if event.get("type").and_then(Value::as_str)==Some("permission.asked")
                            && data.get("sessionID").and_then(Value::as_str)==Some(&self.prepared.id) {
                            approvals.observe(&self.runtime.api,&self.invocation,&self.prepared.id,data)?;
                        }
                        let updates = match displayed.observe(event, self.runtime.api.version) {
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
                            && accepted(&history) { self.check_budget().await?; return Ok(history); }
                        events = self.ready_events().await?;
                    }
                }
                _ = timer.tick() => {
                    let raw = self.runtime.api.history(&self.prepared.id).await?;
                    if let Err(error) = super::budget::validate(self.runtime.api.version, &raw, &self.prepared, self.limits) {
                        self.interrupt().await;
                        return Err(error);
                    }
                    approvals.reconcile(&self.runtime.api,&self.invocation,&self.prepared.id).await?;
                    if let Ok(history) = snapshot(&self.runtime.api, &self.prepared.id, &self.invocation).await
                        && accepted(&history) {
                        return Ok(history);
                    }
                }
                result = approvals.receiver.recv() => {
                    if let Some(result) = result {result?;}
                }
            }
        }
    }
}

async fn connected(events: &mut Events) -> Result<(), ProtocolError> {
    loop {
        let event = events.next().await?.ok_or_else(|| {
            failure(
                Fault::RunRecoveryFailed,
                "OpenCode event stream ended before readiness",
            )
        })?;
        let event = event.get("payload").unwrap_or(&event);
        if event.get("type").and_then(Value::as_str) == Some("server.connected") {
            return Ok(());
        }
    }
}

fn accepted(snapshot: &Snapshot) -> bool {
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
