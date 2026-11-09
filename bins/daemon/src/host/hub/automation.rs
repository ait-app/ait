//! Automation publication: the Host publishes the Workspace Bots the Ait center
//! may reach (each with an enabled Hub channel) on every fresh serve link and after
//! every change.
//!
//! The center opens automation sessions only to a link whose latest publication
//! is enabled, so a Host that never publishes, or publishes `enabled: false`,
//! never receives one.

use std::sync::Arc;
use std::time::Duration;

use bot::service::HubBot;
use model::automation::{MAX_BOTS, PUBLICATION_VERSION, PUBLISH_METHOD};
use model::hub::{CallError, Center, LinkState};
use serde_json::{Value, json};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{COALESCE, MAX_BACKOFF, RETRY};

/// What the center last accepted, and on which logical link.
type Sent = Option<(Option<Uuid>, Value)>;

/// The publication body: reachable Bots, and whether any is active.
fn publication(bots: &[HubBot]) -> Value {
    json!({
        "version": PUBLICATION_VERSION,
        "enabled": bots.iter().any(|bot| bot.state == "active"),
        "bots": bots.iter().take(MAX_BOTS).collect::<Vec<_>>(),
    })
}

/// How one publication attempt ended.
#[derive(Debug, PartialEq, Eq)]
enum Attempt {
    /// Nothing to do, or the center confirmed the publication.
    Settled,
    /// Retry after a pause.
    Retry,
}

/// Publish whenever needed until cancelled.
pub(super) async fn run(
    center: Arc<dyn Center>,
    mut bots: watch::Receiver<Vec<HubBot>>,
    cancel: CancellationToken,
) {
    let mut state = center.state();
    let mut sent: Sent = None;
    let mut backoff = RETRY;
    loop {
        // Release the watch borrows before awaiting the call.
        let link = state.borrow_and_update().clone();
        let current = publication(&bots.borrow_and_update());
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
            local = bots.changed() => {
                if local.is_err() {
                    return;
                }
                tokio::time::sleep(COALESCE).await;
            }
            () = tokio::time::sleep(pause), if pause != Duration::MAX => {}
        }
    }
}

/// Publish `current` if the center supports automation and has not seen it on this link.
async fn publish(
    center: &dyn Center,
    link: &LinkState,
    current: Value,
    sent: &mut Sent,
) -> Attempt {
    let Some(scope) = link.scope.clone().filter(|_| link.online) else {
        return Attempt::Settled;
    };
    if !link.supports(PUBLISH_METHOD)
        || sent
            .as_ref()
            .is_some_and(|(on, published)| *on == link.link && *published == current)
    {
        return Attempt::Settled;
    }
    match center.call(scope, PUBLISH_METHOD, current.clone()).await {
        Ok(_) => {
            *sent = Some((link.link, current));
            Attempt::Settled
        }
        // The link changed or the center dropped the method; state changes retrigger.
        Err(CallError::Unavailable | CallError::Unsupported) => Attempt::Settled,
        Err(error) => {
            tracing::warn!(%error, "Automation publication failed; retrying");
            Attempt::Retry
        }
    }
}

#[cfg(test)]
mod tests;
