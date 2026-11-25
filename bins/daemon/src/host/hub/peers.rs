//! Peer publication: the Host tells the account center whether it accepts calls from Agents on
//! sibling Hosts, on every fresh serve link and after every change.
//!
//! The center starts every fresh logical link as not accepting, whatever an earlier link
//! published, so each new link is published again, `enabled: false` included.

use std::sync::Arc;

use model::hub::{CallError, Center};
use serde_json::{Value, json};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use super::publisher::{self, Attempt, Publication};

/// Center method that records whether this link accepts peer sessions.
const PUBLISH_METHOD: &str = "hub.peers.publish";

/// Version of the publication body.
const PUBLICATION_VERSION: u32 = 1;

const PUBLICATION: Publication = Publication {
    name: "Peer",
    method: PUBLISH_METHOD,
    coalesce: None,
    classify,
};

/// The publication body.
fn body(enabled: bool) -> Value {
    json!({"version": PUBLICATION_VERSION, "enabled": enabled})
}

/// Which failed publications settle. Publishing is idempotent, so a call that timed out or lost
/// its reply on a link that is still live is sent again: otherwise the center would keep the
/// previous value, and after a switch-off keep the link's accepted peer sessions open.
fn classify(error: &CallError, link_gone: bool) -> Attempt {
    match error {
        // The center dropped the method; a fresh link reads its methods again.
        CallError::Unsupported => Attempt::Settled,
        // A fresh link publishes again anyway.
        CallError::Unavailable if link_gone => Attempt::Settled,
        // This link did not declare the peer feature; retrying on it cannot succeed.
        CallError::Rejected(code) if code == "forbidden" => {
            tracing::warn!("Center refused peer publication on a link without the peer feature");
            Attempt::Settled
        }
        CallError::Unavailable
        | CallError::Capacity
        | CallError::Protocol
        | CallError::Rejected(_) => Attempt::Retry,
    }
}

/// Publish whether this Host accepts peer calls whenever needed until cancelled.
///
/// `accepting` is `mcp.enabled && mcp.acceptPeers`, as the API observes it.
pub(super) async fn run(
    center: Arc<dyn Center>,
    accepting: watch::Receiver<bool>,
    cancel: CancellationToken,
) {
    publisher::run(
        &PUBLICATION,
        center,
        accepting,
        |enabled| body(*enabled),
        cancel,
    )
    .await;
}

#[cfg(test)]
mod tests;
