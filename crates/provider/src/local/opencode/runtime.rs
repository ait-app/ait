//! A fresh authenticated helper and its descendants belong to one server session.
use std::{path::Path, process::Stdio, time::Duration};

use crate::local::opencode::types::{Fault, ProtocolError};
use reqwest::{Method, Url};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, BufReader},
    process::{Child, Command},
};
use tokio_util::{sync::CancellationToken, task::AbortOnDropHandle};

use super::{
    failure,
    http::{Api, Version},
};

pub(super) struct Runtime {
    pub(super) api: Api,
    child: Child,
    drain: AbortOnDropHandle<()>,
}

impl Runtime {
    pub(super) async fn spawn(
        binary: &Path,
        cwd: &Path,
        cancellation: &CancellationToken,
    ) -> Result<Self, ProtocolError> {
        Self::spawn_config(binary, cwd, cancellation, None).await
    }

    pub(super) async fn spawn_metadata(
        binary: &Path,
        cwd: &Path,
        agent: &str,
    ) -> Result<Self, ProtocolError> {
        Self::spawn_config(binary, cwd, &CancellationToken::new(), Some(agent)).await
    }

    async fn spawn_config(
        binary: &Path,
        cwd: &Path,
        cancellation: &CancellationToken,
        metadata_agent: Option<&str>,
    ) -> Result<Self, ProtocolError> {
        let version = probe(binary, cwd, cancellation).await?;
        let password = uuid::Uuid::new_v4().simple().to_string();
        let mut command = Command::new(binary);
        command
            .args(["serve", "--hostname", "127.0.0.1", "--port", "0"])
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .env("OPENCODE_SERVER_USERNAME", "opencode");
        if let Some(agent) = metadata_agent {
            command.env(
                "OPENCODE_CONFIG_CONTENT",
                metadata_configuration(version, agent)?,
            );
        }
        match version {
            Version::V1 => {
                command.env("OPENCODE_SERVER_PASSWORD", &password);
            }
            Version::V2 => {
                command.env("OPENCODE_PASSWORD", &password);
            }
        }
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command.spawn().map_err(|_| {
            failure(
                Fault::ProviderFailed,
                "cannot start installed OpenCode executable",
            )
        })?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| failure(Fault::ProviderFailed, "OpenCode stderr pipe unavailable"))?;
        let drain = AbortOnDropHandle::new(tokio::spawn(async move {
            let _ = tokio::io::copy(&mut BufReader::new(stderr), &mut tokio::io::sink()).await;
        }));
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| failure(Fault::ProviderFailed, "OpenCode stdout pipe unavailable"))?;
        let mut stdout = BufReader::new(stdout.take(32_768));
        let result = tokio::select! {
            () = cancellation.cancelled() => Err(failure(Fault::RunCancelled, "OpenCode startup cancelled")),
            result = tokio::time::timeout(Duration::from_secs(30), startup_address(&mut stdout)) =>
                result.unwrap_or_else(|_| Err(failure(Fault::ProviderFailed, "OpenCode startup timed out"))),
        };
        let api = match result
            .and_then(|url| Api::new(version, url, password, cwd.to_string_lossy().into_owned()))
        {
            Ok(api) => api,
            Err(error) => {
                let _ = stop(&mut child).await;
                return Err(error);
            }
        };
        // Drain stdout after readiness as well; neither provider pipe can block execution.
        let stdout = stdout.into_inner().into_inner();
        let drain_stdout = AbortOnDropHandle::new(tokio::spawn(async move {
            let _ = tokio::io::copy(&mut BufReader::new(stdout), &mut tokio::io::sink()).await;
        }));
        let drain = AbortOnDropHandle::new(tokio::spawn(async move {
            let _ = tokio::join!(drain, drain_stdout);
        }));
        let mut runtime = Self { api, child, drain };
        let path = match version {
            Version::V1 => "/global/health",
            Version::V2 => "/api/info",
        };
        let readiness = tokio::select! {
            () = cancellation.cancelled() => Err(failure(Fault::RunCancelled, "OpenCode startup cancelled")),
            result = runtime.api.json(Method::GET, path, None) => result,
        };
        if let Err(error) = readiness {
            let _ = runtime.close().await;
            return Err(error);
        }
        Ok(runtime)
    }

    pub(super) async fn close(&mut self) -> Result<(), ProtocolError> {
        let result = stop(&mut self.child).await;
        self.drain.abort();
        result
    }
}

fn metadata_configuration(version: Version, agent: &str) -> Result<String, ProtocolError> {
    let mut config: serde_json::Value = std::env::var("OPENCODE_CONFIG_CONTENT")
        .ok()
        .map(|value| serde_json::from_str(&value))
        .transpose()
        .map_err(|_| {
            failure(
                Fault::ProviderFailed,
                "invalid OpenCode inline configuration",
            )
        })?
        .unwrap_or_else(|| serde_json::json!({}));
    if !config.is_object() {
        return Err(failure(
            Fault::ProviderFailed,
            "invalid OpenCode inline configuration",
        ));
    }
    let overlay = super::metadata::configuration(version, agent);
    for (key, value) in overlay
        .as_object()
        .expect("metadata configuration is an object")
    {
        config[key] = value.clone();
    }
    Ok(config.to_string())
}

async fn startup_address(stdout: &mut (impl AsyncBufRead + Unpin)) -> Result<Url, ProtocolError> {
    let mut line = String::new();
    loop {
        line.clear();
        if stdout
            .read_line(&mut line)
            .await
            .map_err(|_| failure(Fault::ProviderFailed, "OpenCode startup output interrupted"))?
            == 0
        {
            return Err(failure(
                Fault::ProviderFailed,
                "OpenCode exited before readiness",
            ));
        }
        if let Some((_, address)) = line.split_once("server listening on ") {
            return Url::parse(address.trim()).map_err(|_| {
                failure(
                    Fault::ProviderFailed,
                    "OpenCode returned invalid server address",
                )
            });
        }
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.drain.abort();
        #[cfg(unix)]
        if let Some(id) = self.child.id() {
            let _ = std::process::Command::new("/bin/kill")
                .args(["-KILL", "--", &format!("-{id}")])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

pub(super) async fn probe(
    binary: &Path,
    cwd: &Path,
    cancel: &CancellationToken,
) -> Result<Version, ProtocolError> {
    let mut command = Command::new(binary);
    command
        .arg("--version")
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command
        .spawn()
        .map_err(|_| failure(Fault::ProviderFailed, "OpenCode executable unavailable"))?;
    let mut output = child
        .stdout
        .take()
        .ok_or_else(|| failure(Fault::ProviderFailed, "OpenCode version pipe unavailable"))?
        .take(2049);
    let read = async {
        let mut bytes = Vec::new();
        output
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| failure(Fault::ProviderFailed, "OpenCode version probe interrupted"))?;
        if bytes.len() > 2048 {
            return Err(failure(
                Fault::ProviderFailed,
                "OpenCode version output exceeded limit",
            ));
        }
        let status = child
            .wait()
            .await
            .map_err(|_| failure(Fault::ProviderFailed, "OpenCode version probe failed"))?;
        if !status.success() {
            return Err(failure(
                Fault::ProviderFailed,
                "OpenCode version probe failed",
            ));
        }
        Version::parse(
            std::str::from_utf8(&bytes)
                .map_err(|_| failure(Fault::ProviderFailed, "invalid OpenCode version encoding"))?,
        )
    };
    let result = tokio::select! {
        () = cancel.cancelled() => Err(failure(Fault::RunCancelled, "OpenCode version probe cancelled")),
        result = tokio::time::timeout(Duration::from_secs(5), read) => result
            .unwrap_or_else(|_| Err(failure(Fault::ProviderFailed, "OpenCode version probe timed out"))),
    };
    if result.is_err() {
        let _ = stop(&mut child).await;
    }
    result
}

async fn stop(child: &mut Child) -> Result<(), ProtocolError> {
    #[cfg(unix)]
    if let Some(id) = child.id() {
        let mut signal = Command::new("/bin/kill");
        signal
            .args(["-KILL", "--", &format!("-{id}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let _ = tokio::time::timeout(Duration::from_secs(1), signal.status()).await;
    }
    let _ = child.start_kill();
    tokio::time::timeout(Duration::from_secs(2), child.wait())
        .await
        .map_err(|_| failure(Fault::ProviderFailed, "process did not stop"))?
        .map_err(|_| failure(Fault::ProviderFailed, "process reap failed"))?;
    Ok(())
}
