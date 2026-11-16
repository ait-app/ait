//! Shared bounded structured generation; auxiliary sessions never enter the Agent registry.

mod candidates;
mod prompts;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use domain::agent_runtime::StoredAgentConfig;
use domain::summary::{SummaryError, SummaryRequest};
use serde_json::Value;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::ports::usage_ledger::{
    CallBegin, CallFinish, CallOutcome, CallSource, NoopUsageLedger, UsageLedger,
};
use crate::support::time::now_unix_ms;
use model::summary::{SummaryConfiguration, SummaryGenerator};
use provider_core::ports::agent_session::{AgentClient, AgentSessionSpec};

/// Model-backed summary generator shared by all four wording use cases.
#[derive(Debug)]
pub(crate) struct Generation {
    config: Arc<dyn SummaryConfiguration>,
    clients: BTreeMap<String, Arc<dyn AgentClient>>,
    permits: Semaphore,
    queue: Semaphore,
    cancel: CancellationToken,
    deadline: Duration,
    /// Usage ledger that records every `generate_summary` call.
    ledger: Arc<dyn UsageLedger>,
}

impl Generation {
    /// Use live `config` and registered `clients`, allowing two concurrent 90-second operations.
    /// Clients retain provider credentials in their native authentication stores.
    #[must_use]
    pub(crate) fn new(
        config: Arc<dyn SummaryConfiguration>,
        clients: Vec<Arc<dyn AgentClient>>,
    ) -> Self {
        Self {
            config,
            clients: clients
                .into_iter()
                .map(|client| (client.provider().to_owned(), client))
                .collect(),
            permits: Semaphore::new(2),
            queue: Semaphore::new(32),
            cancel: CancellationToken::new(),
            deadline: Duration::from_secs(90),
            ledger: Arc::new(NoopUsageLedger),
        }
    }

    /// Record every provider call of this generator in `ledger` instead of discarding them.
    #[must_use]
    pub(crate) fn with_usage_ledger(mut self, ledger: Arc<dyn UsageLedger>) -> Self {
        self.ledger = ledger;
        self
    }

    async fn run(&self, request: SummaryRequest) -> Result<Value, SummaryError> {
        let _queued = self
            .queue
            .try_acquire()
            .map_err(|_| SummaryError::Cancelled)?;
        let _permit = self
            .permits
            .acquire()
            .await
            .map_err(|_| SummaryError::Cancelled)?;
        if self.cancel.is_cancelled() || request.context.len() > 1024 * 1024 {
            return Err(SummaryError::Cancelled);
        }
        let config = self.config.clone();
        let input = request.clone();
        let (config, prompt) = tokio::task::spawn_blocking(move || {
            Ok((
                config.current().map_err(|_| SummaryError::Unavailable)?,
                prompts::build(&input, &config.project(&input.cwd)),
            ))
        })
        .await
        .map_err(|_| SummaryError::Unavailable)??;
        let candidates = candidates::resolve(&self.clients, &config, &request).await;
        let schema = prompts::schema(request.kind);
        for candidate in candidates {
            let Some(client) = self.clients.get(&candidate.provider) else {
                continue;
            };
            let spec = AgentSessionSpec {
                provider: candidate.provider,
                cwd: request.cwd.clone(),
                config: StoredAgentConfig {
                    model: candidate.model,
                    thinking_option_id: candidate.thinking_option_id,
                    ..StoredAgentConfig::default()
                },
                confinement: None,
            };
            // Match Paseo's two repair attempts; transport failures move to the next candidate.
            let attempts = if request.kind == domain::summary::SummaryKind::ChatHistory {
                1
            } else {
                3
            };
            for attempt in 0..attempts {
                let text = if attempt == 0 {
                    prompt.clone()
                } else {
                    format!(
                        "{prompt}\n\nThe previous response was invalid. Return only JSON matching the supplied schema."
                    )
                };
                let call = SummaryCall::begin(self.ledger.as_ref(), &spec);
                let output = tokio::time::timeout(
                    Duration::from_secs(25),
                    client.generate_summary(&spec, &text, &schema),
                )
                .await;
                call.finish(if matches!(output, Ok(Ok(_))) {
                    CallOutcome::Completed
                } else {
                    CallOutcome::Failed
                });
                let Ok(Ok(output)) = output else {
                    break;
                };
                if let Some(value) = prompts::parse(request.kind, &output) {
                    return Ok(value);
                }
            }
        }
        Err(SummaryError::Unavailable)
    }
}

/// One `generate_summary` call in the usage ledger, finished as interrupted if dropped unfinished.
struct SummaryCall<'a> {
    /// Ledger that records the call.
    ledger: &'a dyn UsageLedger,
    /// Call that is still running.
    call_id: Option<String>,
}

impl<'a> SummaryCall<'a> {
    /// Begin a `summary` call in `ledger` for the auxiliary session `spec`.
    fn begin(ledger: &'a dyn UsageLedger, spec: &AgentSessionSpec) -> Self {
        let call_id = format!("summary:{}", uuid::Uuid::new_v4());
        ledger.begin_call(CallBegin {
            call_id: call_id.clone(),
            agent_id: String::new(),
            provider: spec.provider.clone(),
            model: spec.config.model.clone(),
            source: CallSource::Summary,
            workspace_id: None,
            started_at: now_unix_ms(),
        });
        Self {
            ledger,
            call_id: Some(call_id),
        }
    }

    /// Finish the call with `outcome`.
    fn finish(mut self, outcome: CallOutcome) {
        self.end(outcome);
    }

    /// Finish the call with `outcome` unless it already finished.
    fn end(&mut self, outcome: CallOutcome) {
        if let Some(call_id) = self.call_id.take() {
            // `generate_summary` returns text only, so the call has no token usage.
            self.ledger.finish_call(CallFinish {
                call_id,
                outcome,
                ended_at: now_unix_ms(),
                token_usage_available: false,
            });
        }
    }
}

impl Drop for SummaryCall<'_> {
    /// A deadline or shutdown dropped the call while the provider was still working.
    fn drop(&mut self) {
        self.end(CallOutcome::Interrupted);
    }
}

#[async_trait::async_trait]
impl SummaryGenerator for Generation {
    async fn generate(&self, request: SummaryRequest) -> Result<Value, SummaryError> {
        tokio::select! {
            biased;
            () = self.cancel.cancelled() => Err(SummaryError::Cancelled),
            result = tokio::time::timeout(self.deadline, self.run(request)) => {
                result.unwrap_or(Err(SummaryError::Unavailable))
            }
        }
    }

    fn shutdown(&self) {
        self.cancel.cancel();
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) mod test_config;
