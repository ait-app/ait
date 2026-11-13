//! Authenticated loopback transport for one owned native transient-generation server.

use std::{net::TcpListener, process::Stdio, time::Duration};

use base64::Engine;
use reqwest::header;
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use tokio::process::Child;
use tokio_util::task::AbortOnDropHandle;

use super::{AgentSessionError, MAX_ANSWER_BYTES, OpenCodeClient, SideRunnerSpec};
use provider_native::process_group::{self, Signal};

const MAX_SCHEMA: usize = 4 * 1024 * 1024;

/// A fresh authenticated endpoint; its address and authorization are never persisted.
#[derive(Debug)]
pub(super) struct Server {
    child: Child,
    group: Option<u32>,
    http: reqwest::Client,
    base: reqwest::Url,
    _stdout: AbortOnDropHandle<()>,
    _stderr: AbortOnDropHandle<()>,
    marker: Option<std::path::PathBuf>,
}

impl Server {
    /// Start a private server and verify the native schema before any generation is admitted.
    pub(super) async fn open(
        client: &OpenCodeClient,
        spec: &SideRunnerSpec,
        configuration: Option<&SecretString>,
    ) -> Result<Self, AgentSessionError> {
        let reserved =
            TcpListener::bind("127.0.0.1:0").map_err(|_| AgentSessionError::Unavailable)?;
        let address = reserved
            .local_addr()
            .map_err(|_| AgentSessionError::Unavailable)?;
        let password = SecretString::from(uuid::Uuid::new_v4().to_string());
        let encoded = base64::engine::general_purpose::STANDARD
            .encode(format!("opencode:{}", password.expose_secret()));
        let mut authorization = header::HeaderValue::from_str(&format!("Basic {encoded}"))
            .map_err(|_| AgentSessionError::Unavailable)?;
        authorization.set_sensitive(true);
        let mut headers = header::HeaderMap::new();
        headers.insert(header::AUTHORIZATION, authorization);
        let http = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(client.deadline)
            .default_headers(headers)
            .build()
            .map_err(|_| AgentSessionError::Unavailable)?;
        let base = reqwest::Url::parse(&format!("http://{address}/"))
            .map_err(|_| AgentSessionError::Unavailable)?;
        let mut command = provider_native::sandbox::command(
            &client.program,
            spec.session.confinement.as_ref(),
            provider_native::sandbox::OPENCODE_STATE,
            &|name| {
                client
                    .environment
                    .get(name)
                    .cloned()
                    .or_else(|| std::env::var(name).ok())
            },
        )?;
        command
            .args([
                "serve",
                "--stdio",
                "--hostname",
                "127.0.0.1",
                "--port",
                &address.port().to_string(),
            ])
            .current_dir(&spec.session.cwd)
            .envs(&client.environment)
            .env("OPENCODE_PASSWORD", password.expose_secret())
            .env("OPENCODE_SERVER_PASSWORD", password.expose_secret())
            .env("OPENCODE_SERVER_USERNAME", "opencode")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(configuration) = configuration {
            command.env("OPENCODE_CONFIG_CONTENT", configuration.expose_secret());
        }
        #[cfg(unix)]
        command.process_group(0);
        drop(reserved);
        let mut child = command
            .spawn()
            .map_err(|_| AgentSessionError::Unavailable)?;
        let group = child.id();
        let stdout = child.stdout.take().ok_or(AgentSessionError::Failed)?;
        let stderr = child.stderr.take().ok_or(AgentSessionError::Failed)?;
        let diagnostics = provider_native::diagnostics::Diagnostics::new("opencode-btw");
        let output_diagnostics = diagnostics.clone();
        let stdout = AbortOnDropHandle::new(tokio::spawn(async move {
            output_diagnostics.drain(stdout).await;
        }));
        let stderr = AbortOnDropHandle::new(tokio::spawn(async move {
            diagnostics.drain(stderr).await;
        }));
        let mut server = Self {
            child,
            group,
            http,
            base,
            _stdout: stdout,
            _stderr: stderr,
            marker: None,
        };
        let ready = server.ready(client.startup_timeout).await;
        if ready.is_err() {
            server.close().await?;
            return Err(AgentSessionError::Unavailable);
        }
        Ok(server)
    }

    async fn ready(&mut self, timeout: Duration) -> Result<(), AgentSessionError> {
        tokio::time::timeout(timeout, async {
            loop {
                let response = self.http.get(self.url("/openapi.json")?).send().await;
                if let Ok(response) = response {
                    // Native 2.x binds its listener before service initialization has settled.
                    if response.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE {
                        tokio::time::sleep(Duration::from_millis(25)).await;
                        continue;
                    }
                    let schema = json(response, MAX_SCHEMA).await?;
                    if schema["paths"]["/api/experimental/generate"]["post"]["operationId"]
                        != "experimental.generate.text"
                    {
                        return Err(AgentSessionError::Unavailable);
                    }
                    return Ok(());
                }
                if self
                    .child
                    .try_wait()
                    .map_err(|_| AgentSessionError::Unavailable)?
                    .is_some()
                {
                    return Err(AgentSessionError::Unavailable);
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .map_err(|_| AgentSessionError::Unavailable)?
    }

    /// Freeze only native provider declarations in the native document order.
    /// Authentication fields stay in redacted memory; executable integrations,
    /// instructions and user configuration files are neither copied nor rewritten.
    pub(super) async fn model_configuration(
        &self,
        spec: &SideRunnerSpec,
    ) -> Result<SecretString, AgentSessionError> {
        let documents = self
            .catalog(self.url("/api/config")?, &spec.session.cwd)
            .await?;
        let documents = documents.as_array().ok_or(AgentSessionError::Unavailable)?;
        let mut providers = json!({});
        for document in documents {
            if document["type"] == "document"
                && let Some(declarations) = document["info"].get("providers")
            {
                merge(&mut providers, declarations);
            }
        }
        Ok(SecretString::from(
            json!({"providers":providers}).to_string(),
        ))
    }

    async fn catalog(&self, url: reqwest::Url, cwd: &str) -> Result<Value, AgentSessionError> {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let response = self
                    .http
                    .get(url.clone())
                    .query(&[("location[directory]", cwd)])
                    .send()
                    .await
                    .map_err(|_| AgentSessionError::Failed)?;
                if response.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                    continue;
                }
                let value = json(response, 32 * 1024 * 1024).await?;
                return Ok(value);
            }
        })
        .await
        .map_err(|_| AgentSessionError::Unavailable)?
    }

    /// Persist ownership before the transient request crosses its send boundary.
    pub(super) fn mark(
        &mut self,
        spec: &SideRunnerSpec,
        request_id: &str,
        topic_id: &str,
    ) -> Result<(), AgentSessionError> {
        let marker = provider_native::auxiliary::path(&spec.directory, request_id);
        provider_native::auxiliary::create(
            &marker,
            request_id,
            topic_id,
            topic_id,
            self.group.ok_or(AgentSessionError::Failed)?,
        )?;
        self.marker = Some(marker);
        Ok(())
    }

    fn url(&self, path: &str) -> Result<reqwest::Url, AgentSessionError> {
        self.base.join(path).map_err(|_| AgentSessionError::Failed)
    }

    /// Invoke only the native tool-free, stateless operation; no generic HTTP gateway exists.
    pub(super) async fn generate(&self, payload: Value) -> Result<String, AgentSessionError> {
        let response = self
            .http
            .post(self.url("/api/experimental/generate")?)
            .header(header::CONTENT_TYPE, "application/json")
            .body(serde_json::to_vec(&payload).map_err(|_| AgentSessionError::Rejected)?)
            .send()
            .await
            .map_err(|_| AgentSessionError::Failed)?;
        let result = json(response, MAX_ANSWER_BYTES * 6 + 1024).await?;
        let text = result["data"]["text"]
            .as_str()
            .filter(|text| !text.trim().is_empty() && text.len() <= MAX_ANSWER_BYTES)
            .ok_or(AgentSessionError::Failed)?;
        Ok(text.to_owned())
    }

    /// Kill and reap this owned group before returning a terminal state.
    pub(super) async fn close(&mut self) -> Result<(), AgentSessionError> {
        process_group::signal(self.group, Signal::Terminate);
        let wait = if let Ok(result) =
            tokio::time::timeout(Duration::from_secs(2), self.child.wait()).await
        {
            result.map_err(|_| AgentSessionError::Failed)
        } else {
            process_group::signal(self.group, Signal::Kill);
            tokio::time::timeout(Duration::from_secs(2), self.child.wait())
                .await
                .map_err(|_| AgentSessionError::Failed)?
                .map_err(|_| AgentSessionError::Failed)
        };
        process_group::signal(self.group.take(), Signal::Kill);
        wait?;
        if let Some(marker) = self.marker.take() {
            provider_native::auxiliary::remove(&marker)?;
        }
        Ok(())
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        process_group::signal(self.group, Signal::Kill);
    }
}

async fn json(mut response: reqwest::Response, budget: usize) -> Result<Value, AgentSessionError> {
    if !response.status().is_success() {
        return Err(AgentSessionError::Failed);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| AgentSessionError::Failed)?
    {
        if bytes.len().saturating_add(chunk.len()) > budget {
            return Err(AgentSessionError::Failed);
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| AgentSessionError::Failed)
}

/// Native provider declarations overlay objects recursively; scalar/array values replace.
fn merge(target: &mut Value, incoming: &Value) {
    if let (Some(target), Some(incoming)) = (target.as_object_mut(), incoming.as_object()) {
        for (key, value) in incoming {
            if key == "headers"
                && let (Some(current), Some(next)) = (
                    target.get_mut(key).and_then(Value::as_object_mut),
                    value.as_object(),
                )
            {
                for (name, value) in next {
                    current.retain(|key, _| !key.eq_ignore_ascii_case(name));
                    current.insert(name.clone(), value.clone());
                }
                continue;
            }
            match target.get_mut(key) {
                Some(current) => merge(current, value),
                None => {
                    target.insert(key.clone(), value.clone());
                }
            }
        }
    } else {
        target.clone_from(incoming);
    }
}
