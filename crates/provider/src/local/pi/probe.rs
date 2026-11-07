use std::time::Duration;

use futures_util::future::BoxFuture;

use super::{PiClient, transport};
use crate::ports::agent_session::AgentSessionError;

/// Run an ephemeral RPC probe after the independently cached version check.
/// `budget` bounds startup, requests and cleanup; `None` keeps per-request deadlines.
/// Returns the request result even if cleanup fails or exhausts the remaining budget.
pub(super) async fn run<T>(
    client: &PiClient,
    cwd: &str,
    confinement: Option<&domain::workspace::sandbox::WriteConfinement>,
    budget: Option<Duration>,
    request: impl for<'a> FnOnce(
        &'a mut transport::Transport,
    ) -> BoxFuture<'a, Result<T, AgentSessionError>>,
) -> Result<T, AgentSessionError> {
    transport::version(client).await?;
    let deadline = budget.map(|budget| tokio::time::Instant::now() + budget);
    let mut transport =
        transport::Transport::spawn(client, cwd, &["--no-session".into()], confinement)?;
    let result = match deadline {
        Some(deadline) => {
            let Ok(result) = tokio::time::timeout_at(deadline, request(&mut transport)).await
            else {
                tracing::warn!(?budget, "Pi discovery RPC request timed out");
                // Drop hands the process to the shutdown worker. There is no remaining
                // budget to await cleanup, and that alone is not a cleanup failure.
                return Err(AgentSessionError::Failed);
            };
            result
        }
        None => request(&mut transport).await,
    };
    let closed = match deadline {
        Some(deadline) => tokio::time::timeout_at(deadline, transport.close())
            .await
            .unwrap_or(Err(AgentSessionError::Failed)),
        None => transport.close().await,
    };
    finish(result, closed)
}

fn finish<T>(
    result: Result<T, AgentSessionError>,
    closed: Result<(), AgentSessionError>,
) -> Result<T, AgentSessionError> {
    if let Err(error) = closed {
        tracing::warn!(?error, "Pi discovery process cleanup failed or timed out");
    }
    result
}

#[cfg(test)]
mod tests;
