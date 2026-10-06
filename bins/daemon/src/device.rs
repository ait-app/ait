//! CLI and process assembly for unattended Host authorization.
mod http;
mod store;

use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, bail};
use chrono::Utc;
use host_link::{CredentialStore, Error, Machine, Pending, State, VERIFICATION_URI};
use secrecy::{ExposeSecret, SecretString};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::config::Login;
use crate::instance::InstanceLease;
pub(super) use http::HttpCenter;
pub(super) use store::Store;

pub(super) async fn login(data: &Path, options: Login) -> anyhow::Result<()> {
    if !cfg!(target_os = "linux") {
        bail!("unattended device authorization currently requires Linux");
    }
    login_with_center(data, options, HttpCenter::new()?).await
}

async fn login_with_center(data: &Path, options: Login, center: HttpCenter) -> anyhow::Result<()> {
    let supplied = read_token(options.token, options.token_stdin)?;
    let instance = InstanceLease::acquire(data)?;
    let store = Store::open(data)?;
    let previous = store.load()?;
    let name = options.name.unwrap_or_else(default_name);
    if name.trim().is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
        bail!("machine name must contain 1–128 bytes without control characters");
    }
    let machine = Machine {
        server_id: instance.server_id,
        display_name: name,
        platform: "linux".to_owned(),
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
    };
    let mut state = match (supplied, previous) {
        (None, Some(state))
            if state.pending.is_some() && !matches!(state.pending, Some(Pending::Refresh(_))) =>
        {
            state
        }
        (supplied, previous) => {
            if previous.as_ref().is_some_and(|s| s.pending.is_some()) {
                bail!(
                    "an unfinished operation exists; recover it or run daemon logout before replacing it"
                );
            }
            let pending = if let Some(token) = supplied {
                Pending::Enrollment {
                    token,
                    request_id: Uuid::new_v4(),
                }
            } else {
                let authorization = center.authorize(&machine).await?;
                Pending::Web {
                    device_code: authorization.device_code,
                    user_code: authorization.user_code,
                    request_id: Uuid::new_v4(),
                    expires_at: Utc::now()
                        + chrono::Duration::seconds(i64::try_from(authorization.expires_in)?),
                    interval: authorization.interval,
                }
            };
            State {
                machine,
                credential: previous.and_then(|s| s.credential),
                pending: Some(pending),
            }
        }
    };
    if state.machine.server_id != instance.server_id {
        bail!("device state belongs to another daemon installation");
    }
    store.save(&state)?;
    if let Some(Pending::Web { user_code, .. }) = &state.pending {
        writeln!(
            std::io::stdout(),
            "Open {VERIFICATION_URI}\nEnter code: {user_code}"
        )?;
    }
    let cancel = CancellationToken::new();
    let result = tokio::select! {
        () = crate::shutdown_signal()? => Err(anyhow::anyhow!("authorization interrupted; rerun daemon login to recover")),
        result = complete_login(&center, &store, &mut state, &cancel) => result.map_err(Into::into),
    };
    result?;
    let binding = &state
        .credential
        .as_ref()
        .context("missing saved device credential")?
        .binding;
    writeln!(
        std::io::stdout(),
        "Authorized Host {}. Start with daemon run --headless.",
        binding.host_id
    )?;
    Ok(())
}

async fn complete_login(
    center: &HttpCenter,
    store: &Store,
    state: &mut State,
    cancel: &CancellationToken,
) -> Result<(), Error> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(600);
    loop {
        let result = match state.pending.as_ref().ok_or(Error::Protocol)? {
            Pending::Enrollment { token, request_id } => {
                center.enroll(token, *request_id, &state.machine).await
            }
            Pending::Web {
                device_code,
                request_id,
                expires_at,
                interval,
                ..
            } => {
                if *expires_at <= Utc::now() {
                    return Err(Error::Denied);
                }
                let seconds = *interval;
                tokio::select! {
                    () = cancel.cancelled() => return Err(Error::Denied),
                    () = tokio::time::sleep(Duration::from_secs(seconds)) => {},
                }
                if *expires_at <= Utc::now() {
                    return Err(Error::Denied);
                }
                center.poll(device_code, *request_id).await
            }
            Pending::Refresh(_) => return Err(Error::Protocol),
        };
        match result {
            Ok(tokens) => {
                if tokens.credential.binding.server_id != state.machine.server_id {
                    return Err(Error::Protocol);
                }
                let mut next = state.clone();
                next.credential = Some(tokens.credential);
                next.pending = None;
                store.save(&next)?;
                *state = next;
                return Ok(());
            }
            Err(Error::Pending) => {}
            Err(Error::SlowDown) => {
                if let Some(Pending::Web { interval, .. }) = &mut state.pending {
                    *interval = (*interval + 5).min(300);
                    store.save(state)?;
                } else {
                    return Err(Error::Protocol);
                }
            }
            Err(Error::Unavailable) => {
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
            Err(error) => return Err(error),
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(Error::Unavailable);
        }
    }
}

pub(super) async fn logout(data: &Path) -> anyhow::Result<()> {
    logout_with_center(data, HttpCenter::new()?).await
}

async fn logout_with_center(data: &Path, center: HttpCenter) -> anyhow::Result<()> {
    let _instance = InstanceLease::acquire(data)?;
    let store = Store::open(data)?;
    let state = store.load()?;
    let unresolved = state.as_ref().is_some_and(|s| {
        matches!(
            s.pending,
            Some(Pending::Web { .. } | Pending::Enrollment { .. })
        )
    });
    let mut online = !unresolved;
    if let Some(credential) = state.and_then(|s| s.credential) {
        online &= center.revoke(&credential.refresh_token).await.is_ok();
    }
    store.clear()?;
    if online {
        writeln!(std::io::stdout(), "Device credentials cleared.")?;
    } else {
        writeln!(
            std::io::stdout(),
            "Local credentials cleared. Center revocation failed; revoke this Host in the web page."
        )?;
    }
    Ok(())
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Connecting,
    Retrying,
    Running,
    Stopped,
    StorageError,
    ReauthorizationRequired,
    RuntimeConflict,
    ProtocolError,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct RuntimeSnapshot {
    phase: Phase,
    binding: Option<host_link::Binding>,
    instance_id: Uuid,
    pid: u32,
    updated_at: chrono::DateTime<Utc>,
}

pub(super) fn status(data: &Path, json_output: bool) -> anyhow::Result<()> {
    let id_path = data.join("server-id");
    // Status is read-only and does not contend with the running credential lock.
    let server_id = std::fs::read_to_string(id_path)
        .ok()
        .and_then(|s| Uuid::parse_str(s.trim()).ok());
    let runtime = std::fs::read(data.join("device/status.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<RuntimeSnapshot>(&bytes).ok());
    let value = serde_json::json!({"mode":"headless", "center":host_link::CENTER,
        "server_id":server_id, "credentials_present":data.join("device/.env").is_file(),
        "runtime_snapshot":runtime});
    if json_output {
        writeln!(std::io::stdout(), "{value}")?;
    } else {
        writeln!(
            std::io::stdout(),
            "{}",
            serde_json::to_string_pretty(&value)?
        )?;
    }
    Ok(())
}

pub(super) fn read_token(
    token: Option<SecretString>,
    stdin: bool,
) -> anyhow::Result<Option<SecretString>> {
    let token = if stdin {
        let mut text = String::new();
        std::io::stdin()
            .take(8193)
            .read_to_string(&mut text)
            .context("read enrollment token from stdin")?;
        if text.len() > 8192 {
            bail!("enrollment token exceeds size limit");
        }
        Some(SecretString::from(text.trim().to_owned()))
    } else {
        token
    };
    if let Some(token) = &token {
        let text = token.expose_secret();
        if text.is_empty()
            || text.len() > 8192
            || !text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
        {
            bail!("invalid enrollment token syntax");
        }
    }
    Ok(token)
}

fn default_name() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "linux-daemon".to_owned())
}

#[cfg(test)]
mod tests;

/// Daemon-owned adapter writes a non-secret snapshot for CLI status without taking the credential lock.
#[derive(Debug)]
pub(super) struct StatusRelay {
    inner: api::ManagedRelayHandle,
    directory: std::path::PathBuf,
    instance: Uuid,
}

impl StatusRelay {
    pub fn new(inner: api::ManagedRelayHandle, data: &Path, instance: Uuid) -> Self {
        Self {
            inner,
            directory: data.join("device"),
            instance,
        }
    }
}

#[async_trait::async_trait]
impl host_link::ManagedRelay for StatusRelay {
    async fn start(&self, session: Uuid, ticket: SecretString) -> Result<(), Error> {
        self.inner.start(session, ticket).await
    }
    async fn active(&self) -> bool {
        self.inner.active().await
    }
    async fn stop(&self) {
        self.inner.stop().await;
    }
    fn report(&self, phase: &'static str, binding: Option<&host_link::Binding>) {
        self.inner.report(phase, binding);
        let value = serde_json::json!({"mode":"managed", "phase":phase, "binding":binding,
            "instance_id":self.instance,"pid":std::process::id(),"updated_at":Utc::now()});
        let temporary = self
            .directory
            .join(format!("status-{}.tmp", Uuid::new_v4()));
        if let Ok(bytes) = serde_json::to_vec(&value) {
            let result = std::fs::write(&temporary, bytes)
                .and_then(|()| std::fs::rename(&temporary, self.directory.join("status.json")));
            if result.is_err() {
                let _ = std::fs::remove_file(&temporary);
            }
        }
    }
}

#[cfg(test)]
mod test_support;
