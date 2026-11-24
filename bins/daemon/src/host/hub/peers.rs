//! Peer publication: the Host tells the account center whether it accepts calls from Agents on
//! sibling Hosts, on every fresh serve link and after every change.
//!
//! The center starts every fresh logical link as not accepting, whatever an earlier link
//! published, so each new link is published again, `enabled: false` included.

use std::sync::Arc;
use std::time::Duration;

use model::hub::{CallError, Center, LinkState};
use serde_json::{Value, json};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{MAX_BACKOFF, RETRY};

/// Center method that records whether this link accepts peer sessions.
const PUBLISH_METHOD: &str = "hub.peers.publish";

/// Version of the publication body.
const PUBLICATION_VERSION: u32 = 1;

/// What the center last accepted, and on which logical link.
type Sent = Option<(Option<Uuid>, bool)>;

/// How one publication attempt ended.
#[derive(Debug, PartialEq, Eq)]
enum Attempt {
    /// Nothing to do, the center confirmed the publication, or retrying cannot help.
    Settled,
    /// Retry after a pause.
    Retry,
}

/// Publish whether this Host accepts peer calls whenever needed until cancelled.
///
/// `accepting` is `mcp.enabled && mcp.acceptPeers`, as the API observes it.
pub(super) async fn run(
    center: Arc<dyn Center>,
    mut accepting: watch::Receiver<bool>,
    cancel: CancellationToken,
) {
    let mut state = center.state();
    let mut sent: Sent = None;
    let mut backoff = RETRY;
    loop {
        // Release the watch borrows before awaiting the call.
        let link = state.borrow_and_update().clone();
        let current = *accepting.borrow_and_update();
        let pause = match publish(center.as_ref(), &link, current, &mut sent).await {
            Attempt::Retry => {
                let pause = backoff;
                backoff = (backoff * 2).min(MAX_BACKOFF);
                pause
            }
            Attempt::Settled => {
                backoff = RETRY;
                Duration::MAX
            }
        };
        tokio::select! {
            () = cancel.cancelled() => return,
            link = state.changed() => if link.is_err() { return },
            local = accepting.changed() => if local.is_err() { return },
            () = tokio::time::sleep(pause), if pause != Duration::MAX => {}
        }
    }
}

/// Publish `enabled` if the center supports it and has not confirmed it on this link.
async fn publish(center: &dyn Center, link: &LinkState, enabled: bool, sent: &mut Sent) -> Attempt {
    let Some(scope) = link.scope.clone().filter(|_| link.online) else {
        return Attempt::Settled;
    };
    if !link.supports(PUBLISH_METHOD) || *sent == Some((link.link, enabled)) {
        return Attempt::Settled;
    }
    match center.call(scope, PUBLISH_METHOD, body(enabled)).await {
        Ok(_) => {
            *sent = Some((link.link, enabled));
            Attempt::Settled
        }
        // The link changed, the center dropped the method, or this link did not declare the
        // peer feature; a later link or value change retriggers.
        Err(CallError::Unavailable | CallError::Unsupported) => Attempt::Settled,
        Err(CallError::Rejected(code)) if code == "forbidden" => {
            tracing::warn!("Center refused peer publication on a link without the peer feature");
            Attempt::Settled
        }
        Err(error) => {
            tracing::warn!(%error, "Peer publication failed; retrying");
            Attempt::Retry
        }
    }
}

/// The publication body.
fn body(enabled: bool) -> Value {
    json!({"version": PUBLICATION_VERSION, "enabled": enabled})
}

#[cfg(test)]
mod tests;
