use std::{sync::Arc, time::Duration};

use anyhow::Context;
use provider::service::agent_execution::AgentExecution;
use serde_json::json;
use tokio_util::sync::CancellationToken;
use voice::{Error, ports::Agents, service::Speech};

#[derive(Debug)]
pub(super) struct NativeAgents(pub AgentExecution);

pub(super) fn compose(
    execution: AgentExecution,
    data_dir: &std::path::Path,
) -> anyhow::Result<Speech> {
    let speech = Speech::from_environment(
        Some(Arc::new(NativeAgents(execution))),
        &data_dir.join("models/local-speech"),
        &std::env::current_exe().context("resolve speech worker executable")?,
    )
    .context("initialize speech backends")?;
    speech.prepare();
    Ok(speech)
}

#[async_trait::async_trait]
impl Agents for NativeAgents {
    async fn realtime(
        &self,
        agent: &str,
        cancel: CancellationToken,
    ) -> Result<Option<model::realtime::Session>, Error> {
        self.0
            .start_realtime(agent, cancel)
            .await
            .map_err(|error| match error {
                provider::rpc::ErrorCode::CatalogBusy => Error::AgentBusy,
                _ => Error::Realtime,
            })
    }

    async fn resolve(&self, identifier: &str) -> Result<String, Error> {
        let result = self
            .0
            .execute("agent.get.request", json!({"agentId":identifier}))
            .await
            .map_err(|_| Error::Agent)?;
        let agent = &result["agent"];
        if !agent["archivedAt"].is_null() || agent["status"] == "archived" {
            return Err(Error::Agent);
        }
        agent["id"].as_str().map(str::to_owned).ok_or(Error::Agent)
    }

    async fn turn(
        &self,
        agent: &str,
        text: &str,
        cancel: CancellationToken,
    ) -> Result<String, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        // Await the admission receipt even if cancellation arrives during send: only an
        // accepted turn belongs to this voice operation and may be interrupted by it.
        let receipt = self
            .0
            .send_voice(agent, text, cancel.clone())
            .await
            .map_err(|_| Error::Agent)?;
        if receipt["accepted"] != true {
            return Err(Error::Agent);
        }
        let turn = receipt["turnId"].as_str().ok_or(Error::Agent)?;
        let result = tokio::select! { biased;
            () = cancel.cancelled() => Err(Error::Cancelled),
            result = self.wait(agent, turn) => result,
        };
        if matches!(result, Err(Error::Cancelled)) {
            let _ = tokio::time::timeout(Duration::from_secs(4), async {
                self.0
                    .execute(
                        "internal.voice.cancel",
                        json!({"agentId":agent,"turnId":turn}),
                    )
                    .await
                    .map_err(|_| Error::Agent)?;
                self.wait(agent, turn).await.map(|_| ())
            })
            .await;
        }
        result
    }
}

impl NativeAgents {
    async fn wait(&self, agent: &str, turn: &str) -> Result<String, Error> {
        loop {
            let result = self
                .0
                .execute(
                    "internal.voice.status",
                    json!({"agentId":agent,"turnId":turn}),
                )
                .await
                .map_err(|_| Error::Agent)?;
            match result["status"].as_str() {
                Some("running") => {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
                Some("idle") => {
                    return Ok(result["lastMessage"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned());
                }
                _ => return Err(Error::Agent),
            }
        }
    }
}
