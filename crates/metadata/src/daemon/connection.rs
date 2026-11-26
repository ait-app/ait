//! Connection-owned request integration.
/// Client methods implemented by this component; consumed by capability discovery.
pub(crate) const METHODS: &[MethodSpec] = &[
    MethodSpec::request("daemon.get_status.request"),
    MethodSpec::request("daemon.host_stats.request"),
    MethodSpec::request("daemon.config.reload.request"),
    MethodSpec::request("daemon.update.request"),
    MethodSpec::request("diagnostics.request"),
    MethodSpec::request("daemon.config.get.request"),
    MethodSpec::request("daemon.config.set.request"),
    MethodSpec::request("server.restart.request"),
    MethodSpec::request("server.shutdown.request"),
];

use model::ErrorCode;
use model::methods::MethodSpec;
use serde_json::Value;

use crate::daemon::rpc;
use crate::dispatch::State as Shared;

pub(crate) async fn dispatch(
    method: &str,
    params: Value,
    state: &Shared,
) -> Result<Value, ErrorCode> {
    if let Some(request) = rpc::lifecycle(method, &params)? {
        state.request_lifecycle(request.intent);
        return Ok(request.value);
    }
    if method == "daemon.host_stats.request" {
        let daemon = state
            .daemon
            .as_ref()
            .ok_or(ErrorCode::UnsupportedCapability)?;
        rpc::validate_host_stats(&params)?;
        daemon.refresh_host_stats(&state.runtime);
        return Ok(rpc::execute(daemon, method, params)?.value);
    }
    let method = method.to_owned();
    let events = state.session_events.clone();
    state
        .run_shared(state.daemon.clone(), ErrorCode::DaemonIo, move |daemon| {
            let executed = rpc::execute(daemon, &method, params)?;
            if let Some(config) = executed.changed_config {
                events.publish(
                    domain::session::protocol::SessionEventKind::DaemonConfig,
                    &serde_json::json!({"status":"daemon_config_changed","config":config}),
                );
            }
            Ok(executed.value)
        })
        .await
}
