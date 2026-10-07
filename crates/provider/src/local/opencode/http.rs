//! Bounded, authenticated loopback HTTP and SSE without proxy or redirect traversal.
use std::{collections::HashSet, time::Duration};

use crate::local::opencode::types::{Fault, Model, ProtocolError};
use reqwest::{Client, Method, Response, Url};
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;

use super::failure;

pub(super) const MAX_BODY: usize = 8 * 1024 * 1024;
const MAX_EVENT: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Version {
    V1,
    V2,
}

impl Version {
    pub(super) fn parse(output: &str) -> Result<Self, ProtocolError> {
        let version = output
            .trim()
            .strip_prefix("opencode ")
            .unwrap_or(output.trim());
        let version = version.strip_prefix('v').unwrap_or(version);
        let numbers = version
            .split(['.', '-', '+'])
            .take(3)
            .map(str::parse::<u32>)
            .collect::<Result<Vec<_>, _>>();
        match numbers.as_deref() {
            Ok([1, _, _]) => Ok(Self::V1),
            Ok([2, minor, patch]) if *minor > 0 || *patch >= 10 => Ok(Self::V2),
            Ok(_) | Err(_) => Err(failure(
                Fault::AgentCapabilityUnsupported,
                "unsupported OpenCode version; require OpenCode 1.x or 2.0.10+",
            )),
        }
    }

    pub(super) fn prefix(self) -> &'static str {
        match self {
            Self::V1 => "",
            Self::V2 => "/api",
        }
    }
}

#[derive(Clone)]
pub(super) struct Api {
    pub(super) version: Version,
    client: Client,
    base: Url,
    password: SecretString,
    cwd: String,
}

impl Api {
    pub(super) fn new(
        version: Version,
        base: Url,
        password: String,
        cwd: String,
    ) -> Result<Self, ProtocolError> {
        if base.scheme() != "http"
            || base.host_str() != Some("127.0.0.1")
            || base.port().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.path() != "/"
            || base.query().is_some()
            || base.fragment().is_some()
        {
            return Err(failure(
                Fault::ProviderFailed,
                "invalid OpenCode loopback address",
            ));
        }
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|_| failure(Fault::ProviderFailed, "OpenCode HTTP client unavailable"))?;
        Ok(Self {
            version,
            client,
            base,
            password: password.into(),
            cwd,
        })
    }

    pub(super) async fn response(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Response, ProtocolError> {
        let mut url = self
            .base
            .join(path)
            .map_err(|_| failure(Fault::ProviderFailed, "invalid OpenCode request path"))?;
        if self.version == Version::V1 && url.path() != "/experimental/session" {
            url.query_pairs_mut().append_pair("directory", &self.cwd);
        }
        let mut request = self
            .client
            .request(method, url)
            .basic_auth("opencode", Some(self.password.expose_secret()));
        if let Some(body) = body {
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body.to_string());
        }
        let response = tokio::time::timeout(Duration::from_secs(30), request.send())
            .await
            .map_err(|_| failure(Fault::RunRecoveryFailed, "OpenCode request timed out"))?
            .map_err(|_| failure(Fault::RunRecoveryFailed, "OpenCode HTTP connection failed"))?;
        if !response.status().is_success() {
            return Err(failure(
                Fault::ProviderFailed,
                "OpenCode rejected the HTTP request",
            ));
        }
        Ok(response)
    }

    pub(super) async fn json(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value, ProtocolError> {
        let response = self.response(method, path, body).await?;
        let bytes = tokio::time::timeout(Duration::from_secs(30), bounded_body(response))
            .await
            .map_err(|_| failure(Fault::RunRecoveryFailed, "OpenCode response timed out"))??;
        if bytes.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&bytes)
            .map_err(|_| failure(Fault::ProviderFailed, "OpenCode returned invalid JSON"))
    }

    pub(super) fn path(&self, id: &str, suffix: &str) -> String {
        format!("{}/session/{id}{suffix}", self.version.prefix())
    }

    pub(super) async fn idle(&self, id: &str) -> Result<bool, ProtocolError> {
        let path = match self.version {
            Version::V1 => "/session/status",
            Version::V2 => "/api/session/active",
        };
        let response = self.json(Method::GET, path, None).await?;
        let statuses = self.data(&response).as_object().ok_or_else(|| {
            failure(
                Fault::ProviderFailed,
                "OpenCode returned invalid execution status",
            )
        })?;
        match (self.version, statuses.get(id)) {
            (_, None) => Ok(true),
            (Version::V1, Some(status)) => match status.get("type").and_then(Value::as_str) {
                Some("idle") => Ok(true),
                Some("busy" | "retry") => Ok(false),
                Some(_) | None => Err(failure(
                    Fault::ProviderFailed,
                    "OpenCode returned unknown execution status",
                )),
            },
            (Version::V2, Some(_)) => Ok(false),
        }
    }

    pub(super) fn data<'a>(&self, response: &'a Value) -> &'a Value {
        match self.version {
            Version::V1 => response,
            Version::V2 => response.get("data").unwrap_or(response),
        }
    }

    pub(super) async fn history(&self, id: &str) -> Result<Vec<Value>, ProtocolError> {
        let path = self.path(id, "/message");
        if self.version == Version::V1 {
            return self
                .json(Method::GET, &path, None)
                .await?
                .as_array()
                .cloned()
                .ok_or_else(|| failure(Fault::ProviderFailed, "invalid OpenCode history"));
        }
        let mut messages = Vec::new();
        let mut cursor = None::<String>;
        let mut cursors = HashSet::new();
        let mut bytes = 0;
        loop {
            let mut url = self
                .base
                .join(&path)
                .map_err(|_| failure(Fault::ProviderFailed, "invalid OpenCode history path"))?;
            url.query_pairs_mut().append_pair("limit", "100");
            if let Some(cursor) = &cursor {
                url.query_pairs_mut().append_pair("cursor", cursor);
            } else {
                url.query_pairs_mut().append_pair("order", "asc");
            }
            let query = format!("{}?{}", url.path(), url.query().unwrap_or_default());
            let page = self.json(Method::GET, &query, None).await?;
            bytes += page.to_string().len();
            if bytes > MAX_BODY {
                return Err(failure(
                    Fault::RunLimitExceeded,
                    "OpenCode history exceeds the output ceiling",
                ));
            }
            messages.extend(
                page.get("data")
                    .and_then(Value::as_array)
                    .ok_or_else(|| failure(Fault::ProviderFailed, "invalid OpenCode history page"))?
                    .iter()
                    .cloned(),
            );
            cursor = match page.pointer("/cursor/next") {
                Some(Value::String(next)) if !next.is_empty() => Some(next.clone()),
                Some(Value::Null) | None => None,
                Some(_) => {
                    return Err(failure(
                        Fault::ProviderFailed,
                        "invalid OpenCode history cursor",
                    ));
                }
            };
            let Some(next) = &cursor else {
                return Ok(messages);
            };
            if !cursors.insert(next.clone()) {
                return Err(failure(
                    Fault::ProviderFailed,
                    "OpenCode history pagination did not advance",
                ));
            }
        }
    }

    pub(super) async fn events(&self) -> Result<Events, ProtocolError> {
        let path = format!("{}/event", self.version.prefix());
        Events::new(self.response(Method::GET, &path, None).await?)
    }

    pub(super) async fn execution(&self, id: &str) -> Result<Option<Value>, ProtocolError> {
        let path = format!("/api/experimental/session/{id}/log?after=0&follow=false");
        let mut events = Events::new(self.response(Method::GET, &path, None).await?)?;
        tokio::time::timeout(Duration::from_secs(30), async {
            let mut latest = None::<Value>;
            let mut total = 0;
            while let Some(event) = events.next().await? {
                total += event.to_string().len();
                if total > MAX_BODY {
                    return Err(failure(
                        Fault::RunLimitExceeded,
                        "OpenCode execution log exceeds the output ceiling",
                    ));
                }
                if !event
                    .get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| kind.starts_with("session.execution."))
                {
                    continue;
                }
                let seq = event
                    .pointer("/durable/seq")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| {
                        failure(Fault::ProviderFailed, "OpenCode execution sequence missing")
                    })?;
                if event.pointer("/data/sessionID").and_then(Value::as_str) != Some(id)
                    || event
                        .pointer("/durable/aggregateID")
                        .and_then(Value::as_str)
                        != Some(id)
                {
                    return Err(failure(
                        Fault::ProviderFailed,
                        "OpenCode execution identity mismatch",
                    ));
                }
                if latest.as_ref().is_none_or(|previous| {
                    previous
                        .pointer("/durable/seq")
                        .and_then(Value::as_u64)
                        .is_some_and(|number| seq > number)
                }) {
                    latest = Some(event);
                }
            }
            Ok(latest)
        })
        .await
        .map_err(|_| {
            failure(
                Fault::RunRecoveryFailed,
                "OpenCode execution reconciliation timed out",
            )
        })?
    }

    pub(super) async fn models(&self) -> Result<Vec<Model>, ProtocolError> {
        let path = match self.version {
            Version::V1 => "/provider".to_owned(),
            Version::V2 => format!("/api/model?{}", location_query(&self.cwd)),
        };
        let response = self.model_catalog(&path).await?;
        let models = match self.version {
            Version::V1 => {
                let connected = response
                    .get("connected")
                    .and_then(Value::as_array)
                    .ok_or_else(|| {
                        failure(
                            Fault::ProviderFailed,
                            "invalid OpenCode connected providers",
                        )
                    })?;
                let providers = response
                    .get("all")
                    .and_then(Value::as_array)
                    .ok_or_else(|| {
                        failure(Fault::ProviderFailed, "invalid OpenCode provider catalog")
                    })?;
                let mut models = Vec::new();
                for provider in providers {
                    let id = provider.get("id").and_then(Value::as_str).ok_or_else(|| {
                        failure(Fault::ProviderFailed, "invalid OpenCode provider identity")
                    })?;
                    if !connected
                        .iter()
                        .any(|connected| connected.as_str() == Some(id))
                    {
                        continue;
                    }
                    if let Some(catalog) = provider.get("models").and_then(Value::as_object) {
                        for (model_id, model) in catalog {
                            models.push(model_definition(id, model_id, model)?);
                        }
                    }
                }
                models
            }
            Version::V2 => self
                .data(&response)
                .as_array()
                .ok_or_else(|| failure(Fault::ProviderFailed, "invalid OpenCode model catalog"))?
                .iter()
                .filter(|model| model.get("enabled").and_then(Value::as_bool) == Some(true))
                .map(|model| {
                    model_definition(
                        required_string(model, "providerID")?,
                        required_string(model, "id")?,
                        model,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
        };
        if models.is_empty() {
            return Err(failure(
                Fault::ProviderFailed,
                "OpenCode has no connected models; authenticate with opencode auth login",
            ));
        }
        Ok(models)
    }

    async fn model_catalog(&self, path: &str) -> Result<Value, ProtocolError> {
        if self.version == Version::V1 {
            return self.json(Method::GET, path, None).await;
        }
        // V2 publishes nonempty, partial models during initial plugin activation.
        // The plugin inventory is published after the initial activation batch, so
        // wait for that boundary before reading models. A quiet/nonempty catalog alone
        // is not evidence that configured providers have been applied.
        let plugins_path = format!("/api/plugin?{}", location_query(&self.cwd));
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let plugins = self.json(Method::GET, &plugins_path, None).await?;
                let plugins = self.data(&plugins).as_array().ok_or_else(|| {
                    failure(Fault::ProviderFailed, "invalid OpenCode plugin inventory")
                })?;
                if plugins.is_empty() {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
                let response = self.json(Method::GET, path, None).await?;
                if !self.data(&response).as_array().is_some_and(Vec::is_empty) {
                    return Ok(response);
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .map_err(|_| {
            failure(
                Fault::ProviderFailed,
                "OpenCode model catalog remained unavailable",
            )
        })?
    }
}

fn model_definition(provider: &str, id: &str, model: &Value) -> Result<Model, ProtocolError> {
    let reasoning_efforts = match model.get("variants") {
        Some(Value::Object(variants)) => variants.keys().cloned().collect(),
        Some(Value::Array(variants)) => variants
            .iter()
            .map(|variant| required_string(variant, "id").map(str::to_owned))
            .collect::<Result<Vec<_>, _>>()?,
        Some(Value::Null) | None => Vec::new(),
        Some(_) => {
            return Err(failure(
                Fault::ProviderFailed,
                "invalid OpenCode model variants",
            ));
        }
    };
    Ok(Model {
        id: format!("{provider}/{id}"),
        name: required_string(model, "name")?.to_owned(),
        reasoning_efforts,
    })
}

fn location_query(cwd: &str) -> String {
    let mut url = Url::parse("http://127.0.0.1/").expect("constant loopback URL");
    url.query_pairs_mut()
        .append_pair("location[directory]", cwd);
    url.query().expect("query inserted").to_owned()
}

pub(super) fn required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str, ProtocolError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| {
            failure(
                Fault::ProviderFailed,
                "OpenCode record is missing a required string",
            )
        })
}

async fn bounded_body(mut response: Response) -> Result<Vec<u8>, ProtocolError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| failure(Fault::RunRecoveryFailed, "OpenCode response interrupted"))?
    {
        if bytes.len() + chunk.len() > MAX_BODY {
            return Err(failure(
                Fault::RunLimitExceeded,
                "OpenCode response exceeds the output ceiling",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

pub(super) struct Events {
    response: Response,
    buffer: Vec<u8>,
}

impl Events {
    pub(super) fn new(response: Response) -> Result<Self, ProtocolError> {
        if !response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/event-stream"))
        {
            return Err(failure(
                Fault::ProviderFailed,
                "OpenCode returned an invalid event stream",
            ));
        }
        Ok(Self {
            response,
            buffer: Vec::new(),
        })
    }

    pub(super) async fn next(&mut self) -> Result<Option<Value>, ProtocolError> {
        loop {
            if let Some((end, separator)) = frame_end(&self.buffer) {
                let frame = self.buffer.drain(..end + separator).collect::<Vec<_>>();
                let frame = std::str::from_utf8(&frame[..end]).map_err(|_| {
                    failure(Fault::ProviderFailed, "invalid OpenCode event encoding")
                })?;
                let data = frame
                    .lines()
                    .filter_map(|line| {
                        line.strip_prefix("data:")
                            .map(|data| data.strip_prefix(' ').unwrap_or(data))
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if data.is_empty() {
                    continue;
                }
                return serde_json::from_str(&data)
                    .map(Some)
                    .map_err(|_| failure(Fault::ProviderFailed, "invalid OpenCode event JSON"));
            }
            let Some(chunk) = self.response.chunk().await.map_err(|_| {
                failure(
                    Fault::RunRecoveryFailed,
                    "OpenCode event stream interrupted",
                )
            })?
            else {
                if self.buffer.iter().all(u8::is_ascii_whitespace) {
                    return Ok(None);
                }
                return Err(failure(
                    Fault::RunRecoveryFailed,
                    "OpenCode event stream ended mid-frame",
                ));
            };
            if self.buffer.len() + chunk.len() > MAX_EVENT {
                return Err(failure(
                    Fault::RunLimitExceeded,
                    "OpenCode event exceeds the output ceiling",
                ));
            }
            self.buffer.extend_from_slice(&chunk);
        }
    }
}

fn frame_end(buffer: &[u8]) -> Option<(usize, usize)> {
    buffer
        .windows(2)
        .position(|pair| pair == b"\n\n")
        .map(|end| (end, 2))
        .or_else(|| {
            buffer
                .windows(4)
                .position(|bytes| bytes == b"\r\n\r\n")
                .map(|end| (end, 4))
        })
}
