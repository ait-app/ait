//! One quota-only native process per launch context; cached reads never lock its transport.
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tokio::io::AsyncReadExt;

use super::super::{CodexClient, Transport};
use crate::ports::account_usage::UsageIdentity;
use crate::ports::agent_session::AgentSessionError;

/// Native account metadata and the independent, serialized quota transport.
#[derive(Debug, Clone, Default)]
pub(in crate::local::codex) struct Shared {
    account: Arc<Mutex<Option<Account>>>,
    process: Arc<tokio::sync::Mutex<Option<Process>>>,
}

#[derive(Debug, Clone)]
pub(super) struct Account {
    pub(super) identity: Option<UsageIdentity>,
    fingerprint: Option<UsageIdentity>,
    cwd: String,
    email: Option<String>,
}

#[derive(Debug)]
struct Process {
    transport: Transport,
    account: Account,
}

impl Shared {
    /// Explicitly close and reap the pooled native process, retaining failed cleanup for retry.
    /// # Errors
    /// Returns native close failure.
    pub(in crate::local::codex) async fn close(&self) -> Result<(), AgentSessionError> {
        let mut slot = self.process.lock().await;
        self.invalidate();
        if let Some(process) = slot.as_mut() {
            process.transport.close().await?;
        }
        *slot = None;
        Ok(())
    }

    /// Resolve an account, reusing metadata only while file-backed credentials are unchanged.
    pub(super) async fn account(
        &self,
        client: &CodexClient,
        cwd: &str,
    ) -> Result<Account, AgentSessionError> {
        let stamp = fingerprint(client).await;
        if let Some(account) = self.cached(cwd, stamp.as_ref()) {
            return Ok(account);
        }
        let mut slot = self.process.lock().await;
        if let Some(account) = self.cached(cwd, stamp.as_ref()) {
            return Ok(account);
        }
        self.invalidate();
        if let Some(process) = slot.as_mut() {
            process.transport.close().await?;
        }
        *slot = None;
        // Retain ownership even if initialization/account inspection is cancelled, so
        // shutdown can explicitly wait for the child instead of relying on runtime drop.
        *slot = Some(Process {
            transport: client.launch_transport(cwd, false)?,
            account: Account {
                identity: None,
                fingerprint: stamp.clone(),
                cwd: cwd.to_owned(),
                email: None,
            },
        });
        let process = slot.as_mut().ok_or(AgentSessionError::Failed)?;
        process.transport.initialize().await?;
        let response = process
            .transport
            .request("account/read", json!({"refreshToken":false}))
            .await?;
        let email = response
            .pointer("/account/email")
            .and_then(Value::as_str)
            .filter(|email| {
                !email.is_empty() && email.len() <= 256 && !email.chars().any(char::is_control)
            })
            .map(str::to_owned);
        let identity = email
            .as_ref()
            .and_then(|_| response.get("account"))
            .and_then(|account| serde_json::to_vec(account).ok())
            .map(|account| UsageIdentity::from_native("codex", &account));
        if stamp != fingerprint(client).await {
            return Err(AgentSessionError::Failed);
        }
        let account = Account {
            identity,
            fingerprint: stamp,
            cwd: cwd.to_owned(),
            email,
        };
        process.account = account.clone();
        *self.account.lock().map_err(|_| AgentSessionError::Failed)? = Some(account.clone());
        Ok(account)
    }

    fn cached(&self, cwd: &str, fingerprint: Option<&UsageIdentity>) -> Option<Account> {
        let fingerprint = fingerprint?;
        self.account
            .lock()
            .ok()?
            .as_ref()
            .filter(|account| {
                account.cwd == cwd && account.fingerprint.as_ref() == Some(fingerprint)
            })
            .cloned()
    }

    /// Read quotas on the process that established this account; discard it on failure/cancel.
    pub(super) async fn report(
        &self,
        client: &CodexClient,
        cwd: &str,
        account: &Account,
    ) -> Result<Value, AgentSessionError> {
        let mut slot = self.process.lock().await;
        let Some(process) = slot.as_mut() else {
            self.invalidate();
            return Err(AgentSessionError::Failed);
        };
        // Install the guard before any await: cancellation while checking credentials
        // must invalidate metadata just like cancellation during the native request.
        let mut attempt = Attempt {
            shared: self,
            completed: false,
        };
        if process.account.identity != account.identity
            || process.account.cwd != cwd
            || process.account.fingerprint != account.fingerprint
            || fingerprint(client).await != account.fingerprint
        {
            self.invalidate();
            return Err(AgentSessionError::Failed);
        }
        let response = process
            .transport
            .request("account/rateLimits/read", json!({}))
            .await?;
        let mut report = super::usage(&response)?;
        if fingerprint(client).await != account.fingerprint {
            return Err(AgentSessionError::Failed);
        }
        if let Some(email) = &account.email {
            report["accountLabel"] = json!(email);
        }
        *self.account.lock().map_err(|_| AgentSessionError::Failed)? = Some(account.clone());
        attempt.completed = true;
        Ok(report)
    }

    fn invalidate(&self) {
        if let Ok(mut account) = self.account.lock() {
            *account = None;
        }
    }
}

// Missing/unknown credential stores deliberately use native identity inspection. Never
// mistake a stale auth.json for the authority when Codex is configured to use a keyring.
/// Fingerprint bounded local file credentials/configuration; unknown stores disable fast reuse.
pub(in crate::local::codex) async fn fingerprint(client: &CodexClient) -> Option<UsageIdentity> {
    let home = client
        .environment
        .variable("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            client
                .environment
                .variable("HOME")
                .or_else(|| client.environment.variable("USERPROFILE"))
                .map(|home| PathBuf::from(home).join(".codex"))
        })?;
    if !home.is_absolute() {
        return None;
    }
    let config = match bounded_file(&home.join("config.toml")).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(_) => return None,
    };
    let config_text = std::str::from_utf8(&config).ok()?;
    let parsed: toml::Value = toml::from_str(config_text).ok()?;
    if parsed
        .get("cli_auth_credentials_store")
        .is_some_and(|store| store.as_str() != Some("file"))
    {
        return None;
    }
    let auth = bounded_file(&home.join("auth.json")).await.ok()?;
    if !serde_json::from_slice::<Value>(&auth).ok()?.is_object() {
        return None;
    }
    let material = serde_json::to_vec(&(
        home,
        config,
        auth,
        client.environment.variable("OPENAI_API_KEY"),
        client.environment.variable("CODEX_API_KEY"),
    ))
    .ok()?;
    Some(UsageIdentity::from_native("codex-launch", &material))
}

struct Attempt<'a> {
    shared: &'a Shared,
    completed: bool,
}
impl Drop for Attempt<'_> {
    fn drop(&mut self) {
        if !self.completed {
            self.shared.invalidate();
        }
    }
}

async fn bounded_file(path: &Path) -> std::io::Result<Vec<u8>> {
    let file = tokio::fs::File::open(path).await?;
    let mut bytes = Vec::new();
    file.take(256 * 1024 + 1).read_to_end(&mut bytes).await?;
    if bytes.len() > 256 * 1024 {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    Ok(bytes)
}

#[cfg(all(test, unix))]
mod tests;
