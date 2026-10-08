//! Private, tool-disabled native requests; transient native history is removed on exit.
use std::{path::Path, time::Duration};

use reqwest::Method;
use serde_json::Value;

use super::runtime::Runtime;
use crate::ports::agent_session::{AgentSessionError, AgentSessionSpec};

struct Request {
    runtime: Option<Runtime>,
    session: Option<String>,
}

impl Request {
    async fn close(&mut self) -> Result<(), AgentSessionError> {
        let Some(mut runtime) = self.runtime.take() else {
            return Ok(());
        };
        let result = if let Some(id) = self.session.take() {
            let _ = runtime.api.interrupt(&id).await;
            runtime
                .api
                .json(Method::DELETE, &runtime.api.path(&id, ""), None)
                .await
                .map(|_| ())
        } else {
            Ok(())
        };
        let closed = runtime.close().await;
        result.and(closed).map_err(|_| AgentSessionError::Failed)
    }
}

impl Drop for Request {
    fn drop(&mut self) {
        let Some(runtime) = self.runtime.take() else {
            return;
        };
        let session = self.session.take();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let mut request = Request {
                    runtime: Some(runtime),
                    session,
                };
                let _ = tokio::time::timeout(Duration::from_secs(5), request.close()).await;
            });
        }
    }
}

pub(super) async fn generate(
    binary: &Path,
    spec: &AgentSessionSpec,
    prompt: &str,
    schema: &Value,
) -> Result<String, AgentSessionError> {
    let (provider, model) = spec
        .config
        .model
        .as_deref()
        .and_then(|model| model.split_once('/'))
        .ok_or(AgentSessionError::Rejected)?;
    let agent = format!("ait-metadata-{}", uuid::Uuid::new_v4().simple());
    let runtime = Runtime::spawn_metadata(binary, Path::new(&spec.cwd), &agent)
        .await
        .map_err(|_| AgentSessionError::Unavailable)?;
    let mut request = Request {
        runtime: Some(runtime),
        session: None,
    };
    let result = run(
        &mut request,
        spec,
        &agent,
        (provider, model),
        &format!("{prompt}\nReturn only JSON matching this schema: {schema}"),
    )
    .await;
    let closed = request.close().await;
    closed?;
    result
}

async fn run(
    request: &mut Request,
    spec: &AgentSessionSpec,
    agent: &str,
    model: (&str, &str),
    prompt: &str,
) -> Result<String, AgentSessionError> {
    let api = &request
        .runtime
        .as_ref()
        .ok_or(AgentSessionError::Failed)?
        .api;
    let parameters = api.metadata_parameters(spec, agent, model, prompt);
    request.session = parameters.id;
    let created = api
        .create_native(&parameters.create)
        .await
        .map_err(|_| AgentSessionError::Failed)?;
    let id = api.data(&created)["id"]
        .as_str()
        .filter(|id| super::session::valid_id(id))
        .ok_or(AgentSessionError::Failed)?
        .to_owned();
    request.session = Some(id.clone());
    api.submit_native(&id, &parameters.prompt)
        .await
        .map_err(|_| AgentSessionError::Failed)?;
    loop {
        let history = api
            .history(&id)
            .await
            .map_err(|_| AgentSessionError::Failed)?;
        if api.idle(&id).await.map_err(|_| AgentSessionError::Failed)?
            && let Some(text) = api.metadata_response(&history)?
        {
            return Ok(text);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
