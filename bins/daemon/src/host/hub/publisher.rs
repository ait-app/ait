//! Per-link publication: the Host publishes one body to one center method on every fresh logical
//! serve link and after every local change, until cancelled.
//!
//! The center keeps such a publication per link, so a new link republishes the current body even
//! when it equals what an earlier link confirmed.

use std::sync::Arc;
use std::time::Duration;

use model::hub::{CallError, Center, LinkState};
use serde_json::Value;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{MAX_BACKOFF, RETRY};

/// What the center last confirmed, and on which logical link.
pub(super) type Sent = Option<(Option<Uuid>, Value)>;

/// How one publication attempt ended.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Attempt {
    /// Nothing to do, the center confirmed the publication, or retrying on this link cannot help.
    Settled,
    /// Retry after a pause.
    Retry,
}

/// One center method published per link.
#[derive(Debug)]
pub(super) struct Publication {
    /// Subject of the retry warning, such as `Automation`.
    pub(super) name: &'static str,
    /// Center method that takes the body.
    pub(super) method: &'static str,
    /// Pause after a local change so a burst of changes publishes once.
    pub(super) coalesce: Option<Duration>,
    /// Decide whether a failed call settles or retries. The flag tells whether the link the call
    /// used has since gone offline or been replaced; a fresh link publishes again anyway.
    pub(super) classify: fn(&CallError, bool) -> Attempt,
}

/// Publish `body(source)` whenever needed until cancelled.
///
/// # Arguments
///
/// * `publication` - the method, coalescing and failure policy
/// * `center` - the serve link's center port
/// * `source` - the local state the body is derived from
/// * `body` - projects the local state into the publication body
/// * `cancel` - ends the loop
pub(super) async fn run<T: Send + Sync>(
    publication: &Publication,
    center: Arc<dyn Center>,
    mut source: watch::Receiver<T>,
    body: impl Fn(&T) -> Value + Send,
    cancel: CancellationToken,
) {
    let mut state = center.state();
    let mut sent: Sent = None;
    let mut backoff = RETRY;
    loop {
        // Release the watch borrows before awaiting the call.
        let link = state.borrow_and_update().clone();
        let current = body(&source.borrow_and_update());
        let pause = match publish(publication, center.as_ref(), &link, current, &mut sent).await {
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
            local = source.changed() => {
                if local.is_err() {
                    return;
                }
                if let Some(coalesce) = publication.coalesce {
                    tokio::time::sleep(coalesce).await;
                }
            }
            () = tokio::time::sleep(pause), if pause != Duration::MAX => {}
        }
    }
}

/// Publish `current` if the center supports the method and has not confirmed it on this link.
pub(super) async fn publish(
    publication: &Publication,
    center: &dyn Center,
    link: &LinkState,
    current: Value,
    sent: &mut Sent,
) -> Attempt {
    let Some(scope) = link.scope.clone().filter(|_| link.online) else {
        return Attempt::Settled;
    };
    if !link.supports(publication.method)
        || sent
            .as_ref()
            .is_some_and(|(on, published)| *on == link.link && *published == current)
    {
        return Attempt::Settled;
    }
    match center
        .call(scope, publication.method, current.clone())
        .await
    {
        Ok(_) => {
            *sent = Some((link.link, current));
            Attempt::Settled
        }
        Err(error) => {
            let attempt = (publication.classify)(&error, link_gone(center, link));
            if attempt == Attempt::Retry {
                tracing::warn!(%error, "{} publication failed; retrying", publication.name);
            }
            attempt
        }
    }
}

/// Whether the link a call used has gone offline or been replaced since.
fn link_gone(center: &dyn Center, used: &LinkState) -> bool {
    let state = center.state();
    let now = state.borrow();
    !now.online || now.link != used.link
}
