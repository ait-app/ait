//! `runtime.hello`: what this machine offers, with no local paths and no credentials.

use std::collections::HashMap;
use std::time::Duration;

use crate::settings::{self, ModelTraits};

use serde::Serialize;
use serde_json::{Value, json};

use crate::ports::{Executor, PortError, Project, Projects};
use crate::wire::{PROTOCOL_VERSION, SESSION_CONTRACT, display, is_model_id, is_token};

const MAX_PROVIDERS: usize = 8;
const MAX_MODELS: usize = 50;
const MAX_PROJECTS: usize = 200;
const BUSY_RETRIES: u32 = 10;

/// Providers that can run sessions.
pub const PROVIDERS: [&str; 2] = ["claude", "codex"];
/// Provider announced as the default when available.
pub const DEFAULT_PROVIDER: &str = "claude";

/// One announced model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Model {
    /// Model ID passed to AIT.
    pub id: String,
    /// Display label.
    pub label: String,
}

/// One announced provider.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Provider {
    /// Provider ID.
    pub id: String,
    /// Selectable models.
    pub models: Vec<Model>,
    /// Whether tool calls stop for approval with no settings filled.
    pub approvals: bool,
    /// Whether agents can write this Bonsai through the dispatch MCP URL.
    pub bonsai_write: bool,
    /// Declared harness settings.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub settings: Vec<Value>,
    /// What each model supports, for validating `effort` and `fast_mode`.
    #[serde(skip)]
    pub traits: HashMap<String, ModelTraits>,
    /// The provider's default model, when it reports one.
    #[serde(skip)]
    pub default_model: Option<String>,
}

impl Provider {
    /// Traits of the model a run uses (`None` = the provider default).
    #[must_use]
    pub fn model_traits(&self, model: Option<&str>) -> Option<&ModelTraits> {
        model
            .map(str::to_owned)
            .or_else(|| self.default_model.clone())
            .and_then(|model| self.traits.get(&model))
    }
}

/// One announced project; `local` maps it back to the host's record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnnouncedProject {
    /// Announced ID (`[A-Za-z0-9._:-]{1,64}`).
    pub id: String,
    /// Display name.
    pub name: String,
    /// Normalized `host/a/b` remote.
    pub git_remote: Option<String>,
    /// Current branch.
    pub branch: Option<String>,
    /// Host project ID, never announced.
    #[serde(skip)]
    pub host_id: String,
}

/// Everything the hello and dispatch validation need.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Offer {
    /// Machine name.
    pub name: String,
    /// Available providers.
    pub providers: Vec<Provider>,
    /// Registered projects.
    pub projects: Vec<AnnouncedProject>,
}

impl Offer {
    /// The provider used when a dispatch names none.
    #[must_use]
    pub fn default_provider(&self) -> Option<&Provider> {
        self.providers
            .iter()
            .find(|provider| provider.id == DEFAULT_PROVIDER)
            .or_else(|| self.providers.first())
    }

    /// Look up an announced provider.
    #[must_use]
    pub fn provider(&self, id: &str) -> Option<&Provider> {
        self.providers.iter().find(|provider| provider.id == id)
    }

    /// Look up an announced project.
    #[must_use]
    pub fn project(&self, id: &str) -> Option<&AnnouncedProject> {
        self.projects.iter().find(|project| project.id == id)
    }

    /// Encode the hello, leaving out projects (then models) from the end until it fits.
    ///
    /// Bad entries are dropped, never fatal (protocol §3): a machine with many long project
    /// names must still connect. Returns the frame and how many entries were left out, or
    /// `None` when even an empty hello does not fit.
    #[must_use]
    pub fn hello_frame(&self, runtime_id: &str) -> Option<(String, usize)> {
        let mut offer = self.clone();
        let mut dropped = 0;
        loop {
            if let Ok(frame) = crate::wire::hello(&offer.hello(runtime_id)) {
                return Some((frame, dropped));
            }
            if !offer.projects.is_empty() {
                let cut = (offer.projects.len() / 10).max(1);
                offer.projects.truncate(offer.projects.len() - cut);
                dropped += cut;
                continue;
            }
            let provider = offer
                .providers
                .iter_mut()
                .max_by_key(|provider| provider.models.len())
                .filter(|provider| !provider.models.is_empty())?;
            provider.models.pop();
            dropped += 1;
        }
    }

    /// Build the `runtime.hello` head.
    #[must_use]
    pub fn hello(&self, runtime_id: &str) -> Value {
        json!({
            "type": "runtime.hello",
            "v": PROTOCOL_VERSION,
            "runtime_id": runtime_id,
            "name": self.name,
            "software": {"name": "ait", "version": env!("CARGO_PKG_VERSION")},
            "session": [SESSION_CONTRACT],
            "default_provider": self.default_provider().map(|provider| provider.id.clone()),
            "providers": self.providers,
            "projects": self.projects,
        })
    }
}

/// Discover providers, models and projects through the host.
///
/// # Arguments
///
/// * `executor` - Agent RPC.
/// * `projects` - Project registry.
/// * `name` - Machine name.
/// * `bonsai_write` - Per provider: whether runs can confine Bonsai writes to the dispatch MCP
///   URL.
///
/// # Errors
///
/// Returns [`PortError`] when the project registry cannot be read.
pub async fn discover(
    executor: &dyn Executor,
    projects: &dyn Projects,
    name: String,
    bonsai_write: &(dyn Fn(&str) -> bool + Sync),
) -> Result<Offer, PortError> {
    let available = execute_retrying(executor, "provider.available.list.request", json!({}))
        .await
        .ok();
    let mut providers = Vec::new();
    for id in PROVIDERS {
        let listed = available.as_ref().and_then(|value| {
            value["providers"].as_array().and_then(|entries| {
                entries
                    .iter()
                    .find(|entry| entry["provider"] == id)
                    .map(|entry| entry["available"] == true)
            })
        });
        if listed != Some(true) {
            continue;
        }
        let listed_models = execute_retrying(
            executor,
            "provider.models.list.request",
            json!({"provider": id}),
        )
        .await
        .unwrap_or_default();
        let modes = execute_retrying(
            executor,
            "provider.modes.list.request",
            json!({"provider": id}),
        )
        .await
        .map(|value| modes(&value))
        .unwrap_or_default();
        let write = bonsai_write(id);
        providers.push(Provider {
            id: id.to_owned(),
            models: models(&listed_models),
            approvals: settings::mode_asks(id, settings::default_mode(id)),
            bonsai_write: write,
            settings: settings::declarations(id, &modes, write),
            traits: traits(&listed_models),
            default_model: listed_models["models"].as_array().and_then(|models| {
                models
                    .iter()
                    .find(|model| model["isDefault"] == true)
                    .and_then(|model| model["id"].as_str().map(str::to_owned))
            }),
        });
    }
    providers.truncate(MAX_PROVIDERS);
    let projects = announce(projects.list().await?);
    Ok(Offer {
        name,
        providers,
        projects,
    })
}

/// Execute a method, retrying only `CatalogBusy` with bounded backoff.
///
/// # Errors
///
/// Returns the last error code.
pub async fn execute_retrying(
    executor: &dyn Executor,
    method: &'static str,
    params: Value,
) -> Result<Value, model::ErrorCode> {
    let mut delay = Duration::from_millis(100);
    let mut attempt = 0;
    loop {
        match executor.execute(method, params.clone()).await {
            Err(model::ErrorCode::CatalogBusy) if attempt < BUSY_RETRIES => {
                attempt += 1;
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(Duration::from_secs(2));
            }
            result => return result,
        }
    }
}

fn modes(value: &Value) -> Vec<settings::Mode> {
    value["modes"]
        .as_array()
        .map(|modes| {
            modes
                .iter()
                .filter_map(|mode| {
                    let id = mode["id"].as_str().filter(|id| is_token(id, 64))?;
                    let label = mode["label"]
                        .as_str()
                        .map(|label| display(label, 128))
                        .filter(|label| !label.is_empty())
                        .unwrap_or_else(|| id.to_owned());
                    Some(settings::Mode {
                        id: id.to_owned(),
                        label,
                    })
                })
                .take(32)
                .collect()
        })
        .unwrap_or_default()
}

fn traits(value: &Value) -> HashMap<String, ModelTraits> {
    value["models"]
        .as_array()
        .map(|models| {
            models
                .iter()
                .filter_map(|model| {
                    let id = model["id"].as_str()?;
                    let thinking = model["thinkingOptions"]
                        .as_array()
                        .map(|options| {
                            options
                                .iter()
                                .filter_map(|option| option["id"].as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default();
                    Some((
                        id.to_owned(),
                        ModelTraits {
                            thinking,
                            fast: model["supportsFastMode"] == true,
                        },
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn models(value: &Value) -> Vec<Model> {
    let mut seen = std::collections::HashSet::new();
    value["models"]
        .as_array()
        .map(|models| {
            models
                .iter()
                .filter_map(|model| {
                    let id = model["id"].as_str().filter(|id| is_model_id(id))?;
                    if !seen.insert(id.to_owned()) {
                        return None;
                    }
                    let label = model["label"]
                        .as_str()
                        .map(|label| display(label, 128))
                        .filter(|label| !label.is_empty())
                        .unwrap_or_else(|| id.to_owned());
                    Some(Model {
                        id: id.to_owned(),
                        label,
                    })
                })
                .take(MAX_MODELS)
                .collect()
        })
        .unwrap_or_default()
}

/// Turn host projects into announced projects (at most 200, IDs in the hello shape).
#[must_use]
pub fn announce(projects: Vec<Project>) -> Vec<AnnouncedProject> {
    let mut announced: Vec<AnnouncedProject> = Vec::new();
    for project in projects {
        let id = announced_id(&project.id);
        if announced.iter().any(|existing| existing.id == id) {
            continue;
        }
        let name = display(&project.name, 128);
        announced.push(AnnouncedProject {
            name: if name.is_empty() { id.clone() } else { name },
            git_remote: project.remote_url.as_deref().and_then(normalize_remote),
            branch: project
                .branch
                .as_deref()
                .map(|branch| display(branch, 255))
                .filter(|branch| !branch.is_empty()),
            id,
            host_id: project.id,
        });
        if announced.len() == MAX_PROJECTS {
            break;
        }
    }
    announced
}

fn announced_id(host_id: &str) -> String {
    if is_token(host_id, 64) {
        return host_id.to_owned();
    }
    format!("p-{}", crate::translate::sha256_hex(host_id, 8))
}

/// Normalize a Git remote to `host/a/b`, matching Bonsai's `normalizeRemote`.
///
/// Scheme, userinfo, port, query and fragment are removed; host and path are lowercased;
/// trailing `/` and `.git` are removed until stable. Hosts without a dot, local paths and
/// anything with other characters give `None`.
#[must_use]
pub fn normalize_remote(remote: &str) -> Option<String> {
    if remote.encode_utf16().count() > 2048 {
        return None;
    }
    let mut rest = remote.trim_matches(char::is_whitespace);
    if let Some(cut) = rest.find(['?', '#']) {
        rest = &rest[..cut];
    }
    let (host, path) = if let Some(after) = scheme_rest(rest) {
        let slash = after.find('/')?;
        let authority = &after[..slash];
        let host = authority
            .rsplit_once('@')
            .map_or(authority, |(_, host)| host);
        let host = strip_port(host);
        (host, &after[slash + 1..])
    } else {
        let slash = rest.find('/');
        let before_slash = slash.map_or(rest, |slash| &rest[..slash]);
        if let Some(at) = before_slash.rfind('@') {
            rest = &rest[at + 1..];
        }
        let colon = rest.find(':');
        let split = rest.find('/');
        match (colon, split) {
            (Some(colon), split) if split.is_none_or(|split| colon < split) => {
                let host = &rest[..colon];
                if host.chars().count() < 2 {
                    return None;
                }
                (host, &rest[colon + 1..])
            }
            (_, Some(split)) => (&rest[..split], &rest[split + 1..]),
            _ => return None,
        }
    };
    if !host.contains('.') {
        return None;
    }
    let host = host.to_ascii_lowercase();
    let mut path = path.to_ascii_lowercase().trim_start_matches('/').to_owned();
    loop {
        let before = path.clone();
        path = path.trim_end_matches('/').to_owned();
        if let Some(stripped) = path.strip_suffix(".git") {
            path = stripped.to_owned();
        }
        if path == before {
            break;
        }
    }
    let host_ok = host
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
        && host.starts_with(|c: char| c.is_ascii_alphanumeric())
        && host.ends_with(|c: char| c.is_ascii_alphanumeric());
    let path_ok = !path.is_empty()
        && path.bytes().all(|b| {
            b.is_ascii_lowercase()
                || b.is_ascii_digit()
                || matches!(b, b'.' | b'_' | b'~' | b'/' | b'-')
        });
    if !host_ok || !path_ok {
        return None;
    }
    let normalized = format!("{host}/{path}");
    (normalized.len() <= 512).then_some(normalized)
}

fn scheme_rest(text: &str) -> Option<&str> {
    let index = text.find("://")?;
    let scheme = &text[..index];
    let mut characters = scheme.chars();
    let valid = characters.next().is_some_and(|c| c.is_ascii_alphabetic())
        && characters.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'));
    valid.then(|| &text[index + 3..])
}

fn strip_port(host: &str) -> &str {
    match host.rfind(':') {
        Some(colon) if host[colon + 1..].bytes().all(|b| b.is_ascii_digit()) => &host[..colon],
        _ => host,
    }
}

/// This machine's name for the hello, without any path.
#[must_use]
pub fn machine_name() -> String {
    let mut command = std::process::Command::new("hostname");
    for name in model::process::private_environment() {
        command.env_remove(name);
    }
    let name = command
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default();
    let name = display(&name, 128);
    if name.is_empty() {
        "ait".to_owned()
    } else {
        name
    }
}

#[cfg(test)]
mod tests;
