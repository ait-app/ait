//! Read-only authentication diagnostics and OAuth quota retrieval. The CLI owns refresh.

mod quota;

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::AsyncReadExt;

use super::{ClaudeClient, config};
use crate::ports::agent_session::AgentSessionError;

#[derive(Debug)]
struct Credentials {
    token: SecretString,
    plan: Option<String>,
    expires_at: Option<i64>,
}

#[derive(Deserialize)]
struct CredentialFile {
    #[serde(rename = "claudeAiOauth")]
    oauth: Option<OAuth>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OAuth {
    access_token: String,
    subscription_type: Option<String>,
    rate_limit_tier: Option<String>,
    expires_at: Option<i64>,
}

impl ClaudeClient {
    pub(super) async fn native_diagnostic(&self) -> Result<String, AgentSessionError> {
        if !config::executable(&self.program) {
            return Ok("Claude Code executable: unavailable".to_owned());
        }
        let mut command = tokio::process::Command::new(&self.program);
        command.args(["auth", "status", "--json"]);
        if let Some(directory) = &self.config_dir {
            command.env("CLAUDE_CONFIG_DIR", directory);
        }
        let status = output(command, self.deadline)
            .await
            .ok()
            .and_then(|raw| serde_json::from_str::<Value>(raw.expose_secret()).ok());
        Ok(diagnostic(status.as_ref()))
    }

    pub(super) async fn native_usage(&self) -> Result<Value, AgentSessionError> {
        let credentials = self.credentials().await?;
        let Some(credentials) = credentials else {
            return Ok(unavailable(
                &json!({"kind":"no_quota","detail":"This login does not report plan usage. Run claude /login to sign in."}),
            ));
        };
        if let Some(expiry) = credentials
            .expires_at
            .and_then(chrono::DateTime::from_timestamp_millis)
            .filter(|expiry| *expiry <= chrono::Utc::now())
        {
            return Ok(unavailable(
                &json!({"kind":"expired","expiresAt":expiry.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),"refreshedBy":"claude /login"}),
            ));
        }
        let response = fetch(
            "https://api.anthropic.com/api/oauth/usage",
            &credentials.token,
            self.deadline,
        )
        .await?;
        if let Some(status) = response["httpStatus"].as_u64() {
            if matches!(status, 401 | 403) {
                return Ok(unavailable(
                    &json!({"kind":"rejected","status":status,"refreshedBy":"claude /login"}),
                ));
            }
            return Err(AgentSessionError::Failed);
        }
        quota::project(&response, credentials.plan.as_deref())
    }

    async fn credentials(&self) -> Result<Option<Credentials>, AgentSessionError> {
        if let Some(token) = self
            .environment_value("CLAUDE_CODE_OAUTH_TOKEN")
            .filter(|token| !token.expose_secret().is_empty())
        {
            if token.expose_secret().len() > 16384
                || token.expose_secret().chars().any(char::is_control)
            {
                return Err(AgentSessionError::Rejected);
            }
            return Ok(Some(Credentials {
                token,
                plan: None,
                expires_at: None,
            }));
        }
        if self
            .environment_value("ANTHROPIC_API_KEY")
            .is_some_and(|token| !token.expose_secret().is_empty())
        {
            return Ok(None);
        }
        let directory = self
            .config_dir
            .clone()
            .or_else(|| std::env::var_os("CLAUDE_HOME").map(PathBuf::from))
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".claude")))
            .ok_or(AgentSessionError::Unavailable)?;
        if let Some(credentials) = read_file(&directory.join(".credentials.json"))? {
            return Ok(Some(credentials));
        }
        let default = std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".claude"));
        if default.as_ref() != Some(&directory) {
            return Ok(None);
        }
        keychain().await
    }

    fn environment_value(&self, key: &str) -> Option<SecretString> {
        self.environment
            .entries()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| SecretString::from(value.to_owned()))
            .or_else(|| std::env::var(key).ok().map(SecretString::from))
    }
}

fn unavailable(problem: &Value) -> Value {
    json!({"providerId":"claude","displayName":"Claude","status":"unavailable",
        "planLabel":null,"windows":[],"problem":problem,"fetchedAt":chrono::Utc::now().to_rfc3339()})
}

fn diagnostic(status: Option<&Value>) -> String {
    let Some(status) = status else {
        return "Claude Code executable: found\nAuthentication inspection: unavailable".to_owned();
    };
    let authentication = if status["loggedIn"] == true {
        match status["authMethod"].as_str() {
            Some("claude.ai" | "oauth") => "Claude account login",
            Some("api_key" | "apiKey") => "API key",
            _ => "authenticated",
        }
    } else {
        "not authenticated"
    };
    format!("Claude Code executable: found\nAuthentication: {authentication}")
}

fn decode(raw: &SecretString) -> Option<Credentials> {
    let parsed: CredentialFile = serde_json::from_str(raw.expose_secret()).ok()?;
    let oauth = parsed.oauth?;
    if oauth.access_token.is_empty()
        || oauth.access_token.len() > 16384
        || oauth.access_token.chars().any(char::is_control)
    {
        return None;
    }
    let plan = oauth
        .subscription_type
        .filter(|label| {
            matches!(
                label.as_str(),
                "pro" | "max" | "team" | "enterprise" | "free"
            )
        })
        .map(|label| {
            let mut label = label[..1].to_ascii_uppercase() + &label[1..];
            if let Some(tier) = oauth
                .rate_limit_tier
                .as_deref()
                .and_then(|tier| tier.rsplit('_').next())
                .filter(|tier| {
                    !tier.is_empty()
                        && tier.len() <= 16
                        && tier.bytes().all(|byte| byte.is_ascii_alphanumeric())
                })
            {
                label.push(' ');
                label.push_str(tier);
            }
            label
        });
    Some(Credentials {
        token: SecretString::from(oauth.access_token),
        plan,
        expires_at: oauth.expires_at,
    })
}

fn read_file(path: &Path) -> Result<Option<Credentials>, AgentSessionError> {
    let metadata = match path.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(AgentSessionError::Unavailable),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 65536 {
        return Err(AgentSessionError::Unavailable);
    }
    let mut raw = String::new();
    std::fs::File::open(path)
        .map_err(|_| AgentSessionError::Unavailable)?
        .take(65537)
        .read_to_string(&mut raw)
        .map_err(|_| AgentSessionError::Unavailable)?;
    let raw = SecretString::from(raw);
    if raw.expose_secret().len() > 65536 {
        return Err(AgentSessionError::Unavailable);
    }
    Ok(decode(&raw))
}

async fn output(
    mut command: tokio::process::Command,
    deadline: Duration,
) -> Result<SecretString, AgentSessionError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|_| AgentSessionError::Unavailable)?;
    let mut stdout = child.stdout.take().ok_or(AgentSessionError::Failed)?;
    let result = tokio::time::timeout(deadline, async {
        let mut raw = String::new();
        (&mut stdout)
            .take(65537)
            .read_to_string(&mut raw)
            .await
            .map_err(|_| AgentSessionError::Failed)?;
        let raw = SecretString::from(raw);
        if raw.expose_secret().len() > 65536 {
            return Err(AgentSessionError::Failed);
        }
        let status = child.wait().await.map_err(|_| AgentSessionError::Failed)?;
        if !status.success() && raw.expose_secret().trim().is_empty() {
            return Err(AgentSessionError::Unavailable);
        }
        Ok(raw)
    })
    .await;
    if let Ok(result) = result {
        result
    } else {
        let _ = child.kill().await;
        Err(AgentSessionError::Unavailable)
    }
}

#[cfg(target_os = "macos")]
async fn keychain() -> Result<Option<Credentials>, AgentSessionError> {
    let user = std::env::var("USER").unwrap_or_default();
    let account = if !user.is_empty()
        && user.len() <= 128
        && user
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        &user
    } else {
        "claude-code-user"
    };
    for args in [
        vec![
            "find-generic-password",
            "-a",
            account,
            "-w",
            "-s",
            "Claude Code-credentials",
        ],
        vec![
            "find-generic-password",
            "-w",
            "-s",
            "Claude Code-credentials",
        ],
    ] {
        let mut command = tokio::process::Command::new("/usr/bin/security");
        command.args(args);
        if let Ok(raw) = output(command, Duration::from_secs(2)).await
            && let Some(credentials) = decode(&raw)
        {
            return Ok(Some(credentials));
        }
    }
    Ok(None)
}

#[cfg(not(target_os = "macos"))]
fn keychain() -> std::future::Ready<Result<Option<Credentials>, AgentSessionError>> {
    std::future::ready(Ok(None))
}

async fn fetch(
    endpoint: &str,
    token: &SecretString,
    deadline: Duration,
) -> Result<Value, AgentSessionError> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(deadline)
        .build()
        .map_err(|_| AgentSessionError::Failed)?;
    let mut response = client
        .get(endpoint)
        .bearer_auth(token.expose_secret())
        .header("Accept", "application/json")
        .header("anthropic-beta", "oauth-2025-04-20")
        .send()
        .await
        .map_err(|_| AgentSessionError::Failed)?;
    if !response.status().is_success() {
        return Ok(json!({"httpStatus":response.status().as_u16()}));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| AgentSessionError::Failed)?
    {
        if bytes.len().saturating_add(chunk.len()) > 256 * 1024 {
            return Err(AgentSessionError::Failed);
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| AgentSessionError::Failed)
}

#[cfg(test)]
mod tests;
