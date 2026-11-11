//! External ACP protocol engine over one unmodified OMP RPC runtime.

mod engine;
mod mapping;
mod protocol;

use serde_json::{Value, json};

use super::{LABEL, OmpClient};

/// Preserve native event fields while routing error payloads through bounded redaction.
/// These are Ait bridge extensions; no capability or extension is required from OMP.
pub(super) fn redact_event(event: &Value) -> Value {
    let safe_error = |error: &Value| {
        let mut failure = crate::local::diagnostics::failure::Failure::default();
        let diagnostic = crate::local::diagnostics::native(error, "omp_bridge");
        failure.observe(LABEL, error, diagnostic);
        json!(failure.message().unwrap_or("OMP native error"))
    };
    let mut event = event.clone();
    if let Some(error) = event.get("error").filter(|error| !error.is_null()) {
        event["error"] = safe_error(error);
    }
    if event["message"]["stopReason"] == "error" {
        let error = safe_error(&event["message"]["errorMessage"]);
        event["message"] = json!({"role":"assistant","timestamp":event["message"]["timestamp"],
            "content":[],"stopReason":"error","errorMessage":error});
    }
    if event["assistantMessageEvent"]["type"] == "error" {
        event["assistantMessageEvent"] =
            json!({"type":"error","error":safe_error(&event["assistantMessageEvent"])});
    }
    event
}

/// Run the bridge on a bounded JSONL stream, sharing the exact host protocol engine.
/// Returns I/O/protocol failures; EOF closes the owned native process without deleting history.
pub(crate) async fn serve<R, W>(client: OmpClient, input: R, output: W) -> std::io::Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    protocol::serve(engine::Engine::new(client), input, output).await
}

#[cfg(all(test, unix))]
mod tests;
