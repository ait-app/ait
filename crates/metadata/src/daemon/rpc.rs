//! Transport-independent request decoding and projection.
//!
//! Server status, mutable configuration and lifecycle request semantics.

use model::LifecycleIntent;
use model::server::Lifecycle;
use serde_json::Value;

use crate::daemon::protocol::{
    ConfigReloadResult, DaemonConfig, DaemonConfigResult, DaemonConfigSetRequest, DaemonStatus,
    DaemonUpdateResult, DiagnosticsResult, EmptyRequest, LifecycleResult, ProviderAvailability,
    RestartRequest,
};
use crate::daemon::service::{Daemon, DaemonError};
use crate::support::error::ErrorCode;
use crate::support::rpc::{decode, encode};

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HostStatsRequest {
    #[serde(default)]
    include_history: bool,
}

/// Response and associated host lifecycle intent.
pub(crate) struct LifecycleRequest {
    /// Serialized acknowledgement.
    pub(crate) value: Value,
    /// Intent to execute through the host's admission/drain path.
    pub(crate) intent: LifecycleIntent,
}

/// Decode a lifecycle method, returning none for ordinary daemon operations.
///
/// # Errors
/// Returns invalid-message errors for malformed lifecycle parameters.
pub(crate) fn lifecycle(
    method: &str,
    params: &Value,
) -> Result<Option<LifecycleRequest>, ErrorCode> {
    let (intent, result) = match method {
        "server.restart.request" => {
            let request: RestartRequest = decode(params.clone())?;
            let reason = request
                .reason
                .filter(|reason| !reason.trim().is_empty())
                .unwrap_or_else(|| "websocket_request".to_owned());
            (
                LifecycleIntent::Restart {
                    reason: reason.clone(),
                },
                LifecycleResult {
                    status: "restart_requested".to_owned(),
                    reason: Some(reason),
                },
            )
        }
        "server.shutdown.request" => {
            let _: EmptyRequest = decode(params.clone())?;
            (
                LifecycleIntent::Shutdown,
                LifecycleResult {
                    status: "shutdown_requested".to_owned(),
                    reason: None,
                },
            )
        }
        _ => return Ok(None),
    };
    Ok(Some(LifecycleRequest {
        value: encode(result, ErrorCode::DaemonIo)?,
        intent,
    }))
}

/// Daemon operation response and the configuration it changed, if any.
pub(crate) struct Executed {
    /// Serialized response.
    pub(crate) value: Value,
    /// Validated configuration after a set or reload, published to observers.
    pub(crate) changed_config: Option<DaemonConfig>,
}

/// Decode and execute one daemon configuration or update operation.
///
/// # Errors
/// Returns validation or configuration storage errors.
pub(crate) fn execute(
    daemon: &mut Daemon,
    method: &str,
    params: Value,
) -> Result<Executed, ErrorCode> {
    match method {
        "daemon.host_stats.request" => {
            if !params.is_object() {
                return Err(ErrorCode::InvalidMessage);
            }
            let request: HostStatsRequest = decode(params)?;
            unchanged(
                daemon
                    .host_stats(request.include_history)
                    .map_err(|_| ErrorCode::DaemonIo)?,
            )
        }
        "daemon.config.get.request" => {
            let _: EmptyRequest = decode(params)?;
            unchanged(DaemonConfigResult {
                config: config(daemon.get_config().map_err(error)?)?,
            })
        }
        "daemon.config.set.request" => {
            let request: DaemonConfigSetRequest = decode(params)?;
            request
                .config
                .validate()
                .map_err(|_| ErrorCode::InvalidMessage)?;
            let patch =
                serde_json::to_value(request.config).map_err(|_| ErrorCode::DaemonConfigInvalid)?;
            let config = config(daemon.set_config(&patch).map_err(error)?)?;
            Ok(Executed {
                value: encode(
                    DaemonConfigResult {
                        config: config.clone(),
                    },
                    ErrorCode::DaemonIo,
                )?,
                changed_config: Some(config),
            })
        }
        "daemon.config.reload.request" => {
            let _: EmptyRequest = decode(params)?;
            let reload = daemon.reload_config().map_err(error)?;
            let config = config(reload.config)?;
            Ok(Executed {
                value: encode(
                    ConfigReloadResult {
                        applied_paths: reload.applied_paths,
                        restart_required_paths: reload.restart_required_paths,
                        override_controlled_paths: reload.override_controlled_paths,
                    },
                    ErrorCode::DaemonIo,
                )?,
                changed_config: Some(config),
            })
        }
        "daemon.update.request" => {
            let _: EmptyRequest = decode(params)?;
            let result = daemon.update_result();
            unchanged(DaemonUpdateResult {
                success: result.success,
                error: result.error,
                previous_version: result.previous_version,
                new_version: result.new_version,
            })
        }
        _ => Err(ErrorCode::MethodNotFound),
    }
}

fn unchanged(result: impl serde::Serialize) -> Result<Executed, ErrorCode> {
    Ok(Executed {
        value: encode(result, ErrorCode::DaemonIo)?,
        changed_config: None,
    })
}

/// Build status or diagnostics from the availability supplied by the Provider owner.
/// # Errors
/// Rejects malformed parameters or methods outside the two snapshot operations.
pub(crate) fn snapshot(
    daemon: &Daemon,
    method: &str,
    params: Value,
    providers: &[ProviderAvailability],
    facts: (&[String], Lifecycle),
) -> Result<Value, ErrorCode> {
    let _: EmptyRequest = decode(params)?;
    let (capabilities, lifecycle) = facts;
    match method {
        "daemon.get_status.request" => {
            let runtime = daemon.runtime();
            encode(
                DaemonStatus {
                    server_id: runtime.server_id.clone(),
                    version: runtime.version.clone(),
                    pid: runtime.pid,
                    node_path: runtime.executable.clone(),
                    started_at: runtime.started_at.clone(),
                    listen: Some(runtime.listen.clone()),
                    relay: None,
                    providers: providers.to_vec(),
                },
                ErrorCode::DaemonIo,
            )
        }
        "diagnostics.request" => {
            let lifecycle = match lifecycle {
                Lifecycle::Ready => "ready",
                Lifecycle::Draining => "draining",
            };
            encode(
                DiagnosticsResult {
                    diagnostic: daemon.diagnostics(capabilities, lifecycle, providers),
                },
                ErrorCode::DaemonIo,
            )
        }
        _ => Err(ErrorCode::MethodNotFound),
    }
}

fn config(value: Value) -> Result<DaemonConfig, ErrorCode> {
    let config: DaemonConfig =
        serde_json::from_value(value).map_err(|_| ErrorCode::DaemonConfigInvalid)?;
    config
        .validate()
        .map_err(|_| ErrorCode::DaemonConfigInvalid)?;
    Ok(config)
}

fn error(error: DaemonError) -> ErrorCode {
    match error {
        DaemonError::InvalidConfig => ErrorCode::DaemonConfigInvalid,
        DaemonError::ConfigIo => ErrorCode::DaemonIo,
    }
}

#[cfg(test)]
mod tests;
