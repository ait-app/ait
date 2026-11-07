//! Automation publication: the Host publishes its automation settings on every
//! fresh serve link and after every local change.
//!
//! The center opens automation sessions only to a link whose latest publication
//! is enabled, so a Host that never publishes, or publishes `enabled: false`,
//! never receives one.

use std::sync::Arc;
use std::time::Duration;

use model::automation::{PUBLISH_METHOD, Settings};
use model::hub::{CallError, Center, LinkState};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{COALESCE, MAX_BACKOFF, RETRY};

/// What the center last accepted, and on which logical link.
type Sent = Option<(Option<Uuid>, Settings)>;

/// How one publication attempt ended.
#[derive(Debug, PartialEq, Eq)]
enum Attempt {
    /// Nothing to do, or the center confirmed the settings.
    Settled,
    /// Retry after a pause.
    Retry,
}

/// Publish whenever needed until cancelled.
pub(super) async fn run(
    center: Arc<dyn Center>,
    mut settings: watch::Receiver<Settings>,
    cancel: CancellationToken,
) {
    let mut state = center.state();
    let mut sent: Sent = None;
    let mut backoff = RETRY;
    loop {
        // Release the watch borrows before awaiting the call.
        let link = state.borrow_and_update().clone();
        let current = settings.borrow_and_update().clone();
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
            local = settings.changed() => {
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
    current: Settings,
    sent: &mut Sent,
) -> Attempt {
    let Some(scope) = link.scope.clone().filter(|_| link.online) else {
        return Attempt::Settled;
    };
    if !link.supports(PUBLISH_METHOD)
        || sent
            .as_ref()
            .is_some_and(|(on, settings)| *on == link.link && *settings == current)
    {
        return Attempt::Settled;
    }
    match center
        .call(scope, PUBLISH_METHOD, current.publication())
        .await
    {
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
