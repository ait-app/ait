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

/// Open the one credential owner even before login, so authenticated API handoff is available.
pub(super) fn load_state(data: &Path, server: Uuid) -> anyhow::Result<(Store, State)> {
    let store = Store::open(data)?;
    let mut state = store.load()?.unwrap_or_else(|| State {
        authorization_id: None,
        disconnected: false,
        machine: Machine {
            server_id: server,
            display_name: system_hostname().unwrap_or_else(|| "ait-daemon".to_owned()),
            platform: PLATFORM.to_owned(),
            app_version: model::BUILD_VERSION.to_owned(),
        },
        credential: None,
        pending: None,
    });
    if state.machine.server_id != server {
        bail!("device state belongs to another installation");
    }
    if state.pending.is_none() {
        model::BUILD_VERSION.clone_into(&mut state.machine.app_version);
        PLATFORM.clone_into(&mut state.machine.platform);
    }
    Ok((store, state))
}

pub(super) async fn login(data: &Path, options: Login) -> anyhow::Result<()> {
    login_with_center(data, options, HttpCenter::new()?).await
}

async fn login_with_center(data: &Path, options: Login, center: HttpCenter) -> anyhow::Result<()> {
    let supplied = read_token(options.token, options.token_stdin)?;
    let instance = InstanceLease::acquire(data)?;
    let store = Store::open(data)?;
    let previous = store.load()?;
    let explicit_name = options.name.is_some();
    let name = options.name.unwrap_or_else(|| {
        previous.as_ref().map_or_else(
            || {
                center
                    .hostname
                    .as_deref()
                    .unwrap_or("ait-daemon")
                    .to_owned()
            },
            |state| state.machine.display_name.clone(),
        )
    });
    if name.trim().is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
        bail!("machine name must contain 1–128 bytes without control characters");
    }
    let machine = Machine {
        server_id: instance.server_id,
        display_name: name,
        platform: PLATFORM.to_owned(),
        app_version: model::BUILD_VERSION.to_owned(),
    };
    let mut state = match previous {
        Some(mut state) => {
            if state.pending.is_none() || options.force {
                if explicit_name {
                    state.machine.display_name = machine.display_name;
                }
                state.machine.platform = machine.platform;
                state.machine.app_version = machine.app_version;
            }
            state
        }
        None => State {
            authorization_id: None,
            disconnected: false,
            machine,
            credential: None,
            pending: None,
        },
    };
    if state.machine.server_id != instance.server_id {
        bail!("device state belongs to another daemon installation");
    }
    let cancel = CancellationToken::new();
    tokio::select! {
        () = crate::shutdown_signal()? => bail!("authorization interrupted; rerun daemon login to recover"),
        result = authenticate(&center, &store, &mut state, (supplied, options.force), &cancel) => result?,
    }
    let binding = &state
        .credential
        .as_ref()
        .context("missing saved device credential")?
        .binding;
    writeln!(
        std::io::stdout(),
        "Authorized Host {}. Start with daemon run.",
        binding.host_id
    )?;
    Ok(())
}

async fn authenticate(
    center: &HttpCenter,
    store: &Store,
    state: &mut State,
    authorization: (Option<SecretString>, bool),
    cancel: &CancellationToken,
) -> anyhow::Result<()> {
    let (supplied, force) = authorization;
    if supplied.is_none() && !force {
        match restore_login(center, store, state, cancel).await {
            Ok(true) => return Ok(()),
            Ok(false) | Err(Error::Unauthorized | Error::Denied) => {}
            Err(error) => return Err(error.into()),
        }
    } else if state.pending.is_some() && !force {
        bail!(
            "an unfinished operation exists; rerun daemon login to recover or use daemon login --force to start a new authorization"
        );
    }
    state.disconnected = false;
    state.authorization_id = None;
    state.pending = Some(if let Some(token) = supplied {
        Pending::Enrollment {
            token,
            request_id: Uuid::new_v4(),
        }
    } else {
        let authorization = center.authorize(&state.machine).await?;
        Pending::Web {
            device_code: authorization.device_code,
            user_code: authorization.user_code,
            request_id: Uuid::new_v4(),
            expires_at: Utc::now()
                + chrono::Duration::seconds(i64::try_from(authorization.expires_in)?),
            interval: authorization.interval,
        }
    });
    store.save(state)?;
    show_approval(state)?;
    complete_login(center, store, state, cancel).await?;
    Ok(())
}

async fn restore_login(
    center: &HttpCenter,
    store: &Store,
    state: &mut State,
    cancel: &CancellationToken,
) -> Result<bool, Error> {
    if matches!(
        state.pending,
        Some(Pending::Web { .. } | Pending::Enrollment { .. })
    ) {
        show_approval(state).map_err(|_| Error::Unavailable)?;
        complete_login(center, store, state, cancel).await?;
        return Ok(true);
    }
    if state.credential.is_none() {
        return Ok(false);
    }
    loop {
        let tokens = host_link::authorization::restore(center, store, state).await?;
        if tokens.access_expires_at > Utc::now() {
            return Ok(true);
        }
        // A replayed receipt keeps its original expiry. Rotate its committed successor.
    }
}

fn show_approval(state: &State) -> std::io::Result<()> {
    if let Some(Pending::Web { user_code, .. }) = &state.pending {
        writeln!(
            std::io::stdout(),
            "Open {VERIFICATION_URI}\nEnter code: {user_code}"
        )?;
    }
    Ok(())
}

async fn complete_login(
    center: &HttpCenter,
    store: &Store,
    state: &mut State,
    cancel: &CancellationToken,
) -> Result<(), Error> {
    tokio::time::timeout(
        Duration::from_secs(600),
        host_link::authorization::complete(center, store, state, cancel),
    )
    .await
    .map_err(|_| Error::Unavailable)?
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
    store.clear()?;
    let mut online = !unresolved;
    if let Some(credential) = state.and_then(|s| s.credential) {
        online &= center.revoke(&credential.refresh_token).await.is_ok();
    }
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
    Unauthorized,
    Disconnected,
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
    let value = serde_json::json!({"mode":"daemon", "center":host_link::CENTER,
        "server_id":server_id, "credentials_present":Store::credentials_present(data)?,
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

/// Platform spelling shared with desktop node registration.
pub(super) const PLATFORM: &str = if cfg!(target_os = "macos") {
    "darwin"
} else {
    std::env::consts::OS
};

#[cfg(unix)]
fn system_hostname() -> Option<String> {
    let system = rustix::system::uname();
    let name = system.nodename().to_str().ok()?.trim();
    (!name.is_empty() && name.len() <= 255 && !name.chars().any(char::is_control))
        .then(|| name.to_owned())
}

#[cfg(not(unix))]
fn system_hostname() -> Option<String> {
    None
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests;

/// Daemon-owned adapter writes a non-secret snapshot for CLI status without taking the credential lock.
#[derive(Debug, Clone)]
pub(super) struct StatusRelay {
    inner: api::managed_relay::ManagedRelayHandle,
    directory: std::path::PathBuf,
    instance: Uuid,
}

impl StatusRelay {
    pub(crate) fn new(
        inner: api::managed_relay::ManagedRelayHandle,
        data: &Path,
        instance: Uuid,
    ) -> Self {
        Self {
            inner,
            directory: data.join("device"),
            instance,
        }
    }
}

impl host_link::ManagedRelay for StatusRelay {
    async fn start(&self, session: Uuid, ticket: SecretString) -> Result<(), Error> {
        self.inner.start(session, ticket).await
    }
    async fn active(&self) -> bool {
        self.inner.active().await
    }
    async fn start_hub(&self, session: Uuid, ticket: SecretString) -> Result<(), Error> {
        self.inner.start_hub(session, ticket).await
    }
    async fn hub_active(&self) -> bool {
        self.inner.hub_active().await
    }
    async fn online(&self) -> bool {
        self.inner.online().await
    }
    async fn hub_online(&self) -> bool {
        self.inner.hub_online().await
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
