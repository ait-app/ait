//! Automation publication: the Host publishes the enabled Webhook channels of the Workspace Bots the Ait center may reach, on every fresh
//! serve link and after every change.
//!
//! The center opens automation sessions only to a link whose latest publication
//! is enabled, so a Host that never publishes, or publishes `enabled: false`,
//! never receives one.

use std::sync::Arc;

use bot::service::WebhookBot;
use model::automation::{MAX_BOTS, PUBLICATION_VERSION, PUBLISH_METHOD};
use model::hub::{CallError, Center};
use serde_json::{Value, json};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use super::COALESCE;
use super::publisher::{self, Attempt, Publication};

const PUBLICATION: Publication = Publication {
    name: "Automation",
    method: PUBLISH_METHOD,
    coalesce: Some(COALESCE),
    classify,
};

/// The publication body: reachable Bots, and whether any is active.
fn publication(bots: &[WebhookBot]) -> Value {
    json!({
        "version": PUBLICATION_VERSION,
        "enabled": bots.iter().any(|bot| bot.state == "active"),
        "bots": bots.iter().take(MAX_BOTS).collect::<Vec<_>>(),
    })
}

/// The link changed or the center dropped the method: state changes retrigger.
fn classify(error: &CallError, _link_gone: bool) -> Attempt {
    match error {
        CallError::Unavailable | CallError::Unsupported => Attempt::Settled,
        CallError::Capacity | CallError::Protocol | CallError::Rejected(_) => Attempt::Retry,
    }
}

/// Publish whenever needed until cancelled.
pub(super) async fn run(
    center: Arc<dyn Center>,
    bots: watch::Receiver<Vec<WebhookBot>>,
    cancel: CancellationToken,
) {
    publisher::run(&PUBLICATION, center, bots, |bots| publication(bots), cancel).await;
}

#[cfg(test)]
mod tests;
