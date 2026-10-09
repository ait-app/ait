//! Shared bounded structured generation; auxiliary sessions never enter the Agent registry.

mod candidates;
mod prompts;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use domain::agent_runtime::StoredAgentConfig;
use domain::summary::{SummaryError, SummaryRequest};
use model::summary::SummaryFuture;
use serde_json::Value;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::ports::agent_session::{AgentClient, AgentSessionSpec};
use crate::summary::{SummaryConfiguration, SummaryGenerator};

/// Model-backed summary generator shared by all four wording use cases.
#[derive(Debug)]
pub struct Generation {
    config: Arc<dyn SummaryConfiguration>,
    clients: BTreeMap<String, Arc<dyn AgentClient>>,
    permits: Semaphore,
    queue: Semaphore,
    cancel: CancellationToken,
    deadline: Duration,
}

impl Generation {
    /// Use live `config` and registered `clients`, allowing two concurrent 90-second operations.
    /// Clients retain provider credentials in their native authentication stores.
    #[must_use]
    pub fn new(config: Arc<dyn SummaryConfiguration>, clients: Vec<Arc<dyn AgentClient>>) -> Self {
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
        }
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
            };
            // Match Paseo's two repair attempts; transport failures move to the next candidate.
            for attempt in 0..3 {
                let text = if attempt == 0 {
                    prompt.clone()
                } else {
                    format!(
                        "{prompt}\n\nThe previous response was invalid. Return only JSON matching the supplied schema."
                    )
                };
                let output = tokio::time::timeout(
                    Duration::from_secs(25),
                    client.generate_summary(&spec, &text, &schema),
                )
                .await;
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

impl SummaryGenerator for Generation {
    fn generate(&self, request: SummaryRequest) -> SummaryFuture<'_> {
        Box::pin(async move {
            tokio::select! {
                biased;
                () = self.cancel.cancelled() => Err(SummaryError::Cancelled),
                result = tokio::time::timeout(self.deadline, self.run(request)) => {
                    result.unwrap_or(Err(SummaryError::Unavailable))
                }
            }
        })
    }

    fn shutdown(&self) {
        self.cancel.cancel();
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) mod test_config;
