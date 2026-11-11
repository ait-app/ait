//! Finish the typed producer-to-download handoff using the existing Files grant and relay lifetime.

use std::sync::Arc;
use std::time::Duration;

use model::ErrorCode;
use provider::ports::timeline_export::{ExportedTimeline, Format, Scope};
use serde::Serialize;
use serde_json::Value;

use crate::Shared;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Receipt {
    agent_id: String,
    format: Format,
    scope: Scope,
    source: &'static str,
    token: String,
    file_name: String,
    mime_type: String,
    size: u64,
    entries: u32,
    expires_at: String,
}

pub(super) async fn register(state: &Shared, export: ExportedTimeline) -> Result<Value, ErrorCode> {
    let token = uuid::Uuid::new_v4().to_string();
    let receipt = Receipt {
        agent_id: export.artifact.agent_id.clone(),
        format: export.format,
        scope: export.scope,
        source: "persisted_timeline",
        token: token.clone(),
        file_name: export.artifact.file_name.clone(),
        mime_type: export.artifact.mime_type.clone(),
        size: export.artifact.size,
        entries: export.entries,
        expires_at: (chrono::Utc::now() + chrono::Duration::seconds(60)).to_rfc3339(),
    };
    let agent_id = receipt.agent_id.clone();
    let files = Arc::clone(
        state
            .filesystem
            .files
            .as_ref()
            .ok_or(ErrorCode::NotImplemented)?,
    );
    let runtime = Arc::clone(&state.runtime);
    state
        .run(
            Some(Arc::clone(&files)),
            ErrorCode::AgentIo,
            move |service| {
                let finished = service
                    .issue_artifact(token.clone(), export.artifact)
                    .map_err(|_| ErrorCode::AgentIo)?;
                let shutdown = runtime.cancellation.clone();
                // Install expiry in the same admitted job as the grant: a dropped response future must
                // not leave a private temporary file without its lifetime owner.
                runtime.tasks.spawn(async move {
                    tokio::select! {
                        () = finished.cancelled() => return,
                        () = shutdown.cancelled() => {},
                        () = tokio::time::sleep(Duration::from_secs(60)) => {},
                    }
                    // Cleanup must run even when short-job admission is full.
                    let cleanup = tokio::task::spawn_blocking(move || {
                        let mut files = files.lock().map_err(|_| ErrorCode::AgentIo)?;
                        files.release_artifact(&agent_id, &token);
                        Ok::<_, ErrorCode>(())
                    })
                    .await;
                    if !matches!(cleanup, Ok(Ok(()))) {
                        tracing::warn!("Failed to release expired Timeline export grant");
                    }
                });
                Ok(())
            },
        )
        .await?;
    serde_json::to_value(receipt).map_err(|_| ErrorCode::AgentIo)
}

pub(super) async fn release(
    state: &Shared,
    agent_id: String,
    token: String,
) -> Result<Value, ErrorCode> {
    #[derive(Serialize)]
    struct Released {
        released: bool,
    }
    let files = Arc::clone(
        state
            .filesystem
            .files
            .as_ref()
            .ok_or(ErrorCode::NotImplemented)?,
    );
    // Explicit cancellation must clean up even while other requests fill the short-job budget.
    let released = tokio::task::spawn_blocking(move || {
        let mut files = files.lock().map_err(|_| ErrorCode::AgentIo)?;
        Ok::<_, ErrorCode>(files.release_artifact(&agent_id, &token))
    })
    .await
    .map_err(|_| ErrorCode::AgentIo)??;
    serde_json::to_value(Released { released }).map_err(|_| ErrorCode::AgentIo)
}

#[cfg(test)]
mod tests;
