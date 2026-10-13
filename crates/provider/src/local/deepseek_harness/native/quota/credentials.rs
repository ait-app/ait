//! Resolve only DSH's `DeepSeek` credential reference, retaining native launch-layer precedence.
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use regex::Regex;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::json;
use tokio::io::AsyncReadExt;

use super::super::http::Api;
use crate::{
    local::{deepseek_harness::DeepSeekHarnessClient, metadata_process},
    ports::agent_session::AgentSessionError,
};

const MAX_FILE: u64 = 1024 * 1024;

/// Ephemeral launch layers for one Host; secret values are redacted and never persisted by Ait.
#[derive(Debug)]
pub(in crate::local::deepseek_harness::native) struct Context {
    client: DeepSeekHarnessClient,
    cwd: PathBuf,
    home: PathBuf,
    environment: BTreeMap<String, SecretString>,
    project: BTreeMap<String, SecretString>,
    user: BTreeMap<String, SecretString>,
}

impl Context {
    /// Freeze the same process/project/home environment as the native Host; stored keys stay live.
    /// Returns bounded file errors without including paths or source text in diagnostics.
    pub(in crate::local::deepseek_harness::native) async fn capture(
        client: &DeepSeekHarnessClient,
        cwd: &str,
    ) -> Result<Self, AgentSessionError> {
        let environment = std::env::vars()
            .map(|(key, value)| (key, SecretString::from(value)))
            .chain(
                client
                    .environment
                    .entries()
                    .map(|(key, value)| (key.to_owned(), SecretString::from(value))),
            )
            .collect::<BTreeMap<_, _>>();
        let home = environment
            .get("DSH_HOME")
            .filter(|value| !value.expose_secret().is_empty())
            .map(|value| PathBuf::from(value.expose_secret()))
            .or_else(|| {
                environment
                    .get("HOME")
                    .map(|value| PathBuf::from(value.expose_secret()).join(".dsh"))
            })
            .ok_or(AgentSessionError::Unavailable)?;
        let cwd = PathBuf::from(cwd);
        let home = absolute(&cwd, home);
        let project = dotenv(&read(&cwd.join(".env")).await?);
        let user = if home == cwd {
            BTreeMap::new()
        } else {
            dotenv(&read(&home.join(".env")).await?)
        };
        Ok(Self {
            client: client.clone(),
            cwd,
            home,
            environment,
            project,
            user,
        })
    }

    fn environment_value(&self, name: &str) -> Option<&str> {
        self.environment
            .get(name)
            .or_else(|| self.project.get(name))
            .or_else(|| self.user.get(name))
            .map(ExposeSecret::expose_secret)
    }

    /// Read the composed plugin identities, then ask the running Host which credential source wins.
    /// No key is returned if the official `DeepSeek` route is absent or unconfigured.
    pub(in crate::local::deepseek_harness::native) async fn resolve(
        &self,
        api: &Api,
    ) -> Result<Option<SecretString>, AgentSessionError> {
        let mut command = self.client.command();
        command
            .current_dir(&self.cwd)
            .args(["--profile", "web", "--dump-config"]);
        let raw = SecretString::from(
            tokio::time::timeout(self.client.deadline, metadata_process::output(&mut command))
                .await
                .map_err(|_| AgentSessionError::Failed)??,
        );
        let plugins = plugins(&raw)?;
        let routes: Vec<_> = plugins
            .iter()
            .filter(|entry| entry.name == "@deepseek-ai/dsh-llm-deepseek-api-key")
            .collect();
        let Some(route) = routes.first() else {
            return Ok(None);
        };
        if routes.len() != 1 {
            return Err(AgentSessionError::Failed);
        }
        let Some(settings) = api.optional_call("settings/describe", json!({})).await? else {
            return Ok(None);
        };
        let namespaces = settings["namespaces"]
            .as_array()
            .ok_or(AgentSessionError::Failed)?;
        let Some(section) = namespaces
            .iter()
            .find(|section| section["ns"].as_str() == Some(&route.id))
        else {
            return Ok(None);
        };
        let reference = section["value"]["apiKeyEnv"]
            .as_str()
            .filter(|value| valid_reference(value))
            .ok_or(AgentSessionError::Failed)?;
        let base = section["value"]["baseURL"]
            .as_str()
            .or_else(|| self.environment_value("DEEPSEEK_BASE_URL"))
            .unwrap_or("https://api.deepseek.com/anthropic");
        if !official(base) {
            return Err(AgentSessionError::Unavailable);
        }
        let info = api
            .call("credentials/describe", json!({"refs":[reference]}))
            .await?;
        match info[reference]["configured"].as_bool() {
            Some(false) => return Ok(None),
            Some(true) => {}
            None => return Err(AgentSessionError::Failed),
        }
        let key = match info[reference]["source"].as_str() {
            Some("env") => self.environment.get(reference).cloned(),
            Some("project-env") => self.project.get(reference).cloned(),
            Some("user-env") => self.user.get(reference).cloned(),
            Some("file") => {
                let stores: Vec<_> = plugins
                    .iter()
                    .filter(|entry| entry.name == "@deepseek-ai/dsh-credentials-local")
                    .collect();
                if stores.len() != 1 {
                    return Err(AgentSessionError::Unavailable);
                }
                let config = &stores[0].config;
                let home = optional_path(config, "dshHome")?
                    .map_or_else(|| self.home.clone(), |path| absolute(&self.cwd, path));
                let path = optional_path(config, "path")?.map_or_else(
                    || home.join(".credentials.yaml"),
                    |path| absolute(&self.cwd, path),
                );
                stored(&read(&path).await?, reference)?
            }
            _ => return Err(AgentSessionError::Unavailable),
        };
        key.filter(|key| {
            !key.expose_secret().is_empty()
                && key.expose_secret().len() <= 16384
                && !key.expose_secret().chars().any(char::is_control)
        })
        .map(Some)
        .ok_or(AgentSessionError::Failed)
    }
}

#[derive(Deserialize)]
struct Plugin {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    config: serde_yaml_ng::Value,
}

fn plugins(raw: &SecretString) -> Result<Vec<Plugin>, AgentSessionError> {
    // DSH emits executable !!js tags. Preserve them as local tags so Serde cannot silently
    // treat an unknown standard tag as a plain path. Nothing in this reader evaluates JS.
    let tagged = SecretString::from(format!("%TAG !! !\n---\n{}", raw.expose_secret()));
    serde_yaml_ng::from_str(tagged.expose_secret()).map_err(|_| AgentSessionError::Failed)
}

fn optional_path(
    value: &serde_yaml_ng::Value,
    key: &str,
) -> Result<Option<PathBuf>, AgentSessionError> {
    let Some(value) = value.get(key) else {
        return Ok(None);
    };
    // Executable configuration tags are DSH-owned; never evaluate or guess their result.
    match value {
        serde_yaml_ng::Value::String(text) if !text.is_empty() => Ok(Some(PathBuf::from(text))),
        _ => Err(AgentSessionError::Unavailable),
    }
}

fn absolute(cwd: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

async fn read(path: &Path) -> Result<SecretString, AgentSessionError> {
    let file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SecretString::from(String::new()));
        }
        Err(_) => return Err(AgentSessionError::Failed),
    };
    let mut raw = String::new();
    file.take(MAX_FILE + 1)
        .read_to_string(&mut raw)
        .await
        .map_err(|_| AgentSessionError::Failed)?;
    if raw.len() as u64 > MAX_FILE {
        return Err(AgentSessionError::Failed);
    }
    Ok(raw.into())
}

fn stored(raw: &SecretString, reference: &str) -> Result<Option<SecretString>, AgentSessionError> {
    let value: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(raw.expose_secret()).map_err(|_| AgentSessionError::Failed)?;
    if value["version"].as_u64() != Some(1) {
        return Err(AgentSessionError::Failed);
    }
    let key = &value["refs"][reference];
    if key.is_null() {
        return Ok(None);
    }
    key.as_str()
        .map(|key| Some(SecretString::from(key)))
        .ok_or(AgentSessionError::Failed)
}

fn valid_reference(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.bytes().enumerate().all(|(index, byte)| {
            byte == b'_' || byte.is_ascii_alphabetic() || (index > 0 && byte.is_ascii_digit())
        })
}

fn official(base: &str) -> bool {
    reqwest::Url::parse(base).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some("api.deepseek.com")
            && url.port_or_known_default() == Some(443)
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
    })
}

fn dotenv(raw: &SecretString) -> BTreeMap<String, SecretString> {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    let pattern=PATTERN.get_or_init(||Regex::new(r#"(?m)^[\t ]*(?:export[\t ]+)?([A-Za-z_][A-Za-z0-9_]*)[\t ]*=[\t ]*(?:'([^']*)'|"([^"]*)"|`([^`]*)`|([^\r\n#]*))"#).expect("static Node dotenv pattern"));
    pattern
        .captures_iter(raw.expose_secret())
        .map(|captures| {
            let value = if let Some(value) = captures.get(3) {
                value.as_str().replace("\\n", "\n")
            } else if let Some(value) = captures.get(2).or_else(|| captures.get(4)) {
                value.as_str().to_owned()
            } else {
                captures[5].trim().to_owned()
            };
            (captures[1].to_owned(), SecretString::from(value))
        })
        .collect()
}

#[cfg(test)]
mod tests;
