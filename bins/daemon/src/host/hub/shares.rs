//! Share publication: the Host republishes its complete share list on every fresh
//! serve link and after every local change, and applies the center's outcomes.
//!
//! The center checks each publication and answers with share identities. Later changes
//! to group membership or direct-share recipients arrive as `share_status`.
//! Guest sessions are only routed to a link after it published, so a Host that
//! never publishes never receives one.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use model::hub::{CallError, Center, LinkState};
use model::share::{Audience, Outcome, PUBLISH_METHOD, Published, Rejected, ShareBook};
use serde_json::json;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{COALESCE, MAX_BACKOFF, RETRY};

/// The local share list settled on one logical link. A legacy center receives its group entries.
#[derive(Debug, PartialEq)]
struct Sent {
    link: Option<Uuid>,
    revision: u64,
    entries: Vec<Published>,
}

/// Publication state across links; revisions only grow, also across restarts.
#[derive(Debug, Default)]
struct Publisher {
    sent: Option<Sent>,
    revision: u64,
    legacy_link: Option<Uuid>,
}

impl Publisher {
    /// The revision for `entries`: unchanged content keeps its revision so a retry
    /// or a fresh link is idempotent; new content gets a larger one. Wall-clock
    /// milliseconds keep revisions increasing after a daemon restart.
    fn revision_for(&mut self, entries: &[Published]) -> u64 {
        if let Some(sent) = &self.sent
            && sent.entries == entries
        {
            return sent.revision;
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| {
                u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
            });
        self.revision = (self.revision + 1).max(now);
        self.revision
    }

    /// Whether `entries` still need to reach the center on `link`.
    fn pending(&self, link: Option<Uuid>, entries: &[Published]) -> bool {
        self.sent
            .as_ref()
            .is_none_or(|sent| sent.link != link || sent.entries != entries)
    }
}

/// How one publication attempt ended.
#[derive(Debug, PartialEq, Eq)]
enum Attempt {
    /// Nothing to do, or the center confirmed the list.
    Settled,
    /// Retry after a pause.
    Retry,
}

/// Publish whenever needed until cancelled.
pub(super) async fn run(
    center: Arc<dyn Center>,
    book: Arc<dyn ShareBook>,
    cancel: CancellationToken,
) {
    let mut state = center.state();
    let mut changes = book.changes();
    let mut status = center.share_status();
    let mut publisher = Publisher::default();
    let mut backoff = RETRY;
    loop {
        // Release the watch borrow before awaiting the call.
        let current = state.borrow_and_update().clone();
        let attempt = publish(center.as_ref(), book.as_ref(), &current, &mut publisher).await;
        let pause = if attempt == Attempt::Retry {
            let pause = backoff;
            backoff = (backoff * 2).min(MAX_BACKOFF);
            pause
        } else {
            backoff = RETRY;
            Duration::MAX
        };
        tokio::select! {
            () = cancel.cancelled() => return,
            link = state.changed() => if link.is_err() { return },
            local = changes.changed() => {
                if local.is_err() {
                    return;
                }
                tokio::time::sleep(COALESCE).await;
            }
            pushed = status.changed() => {
                if pushed.is_err() {
                    return;
                }
                let outcome = status.borrow_and_update().clone();
                if let Some(outcome) = outcome {
                    book.apply(outcome);
                }
            }
            () = tokio::time::sleep(pause), if pause != Duration::MAX => {}
        }
    }
}

/// Publish the current list if the center supports shares and has not seen it on this link.
async fn publish(
    center: &dyn Center,
    book: &dyn ShareBook,
    state: &LinkState,
    publisher: &mut Publisher,
) -> Attempt {
    let Some(scope) = state.scope.clone().filter(|_| state.online) else {
        return Attempt::Settled;
    };
    if !state.supports(PUBLISH_METHOD) {
        return Attempt::Settled;
    }
    // Visitors ride along but never trigger a publication: the center adds the
    // visitors it admits itself.
    let publication = book.publication();
    let entries = publication.shares;
    if !publisher.pending(state.link, &entries) {
        return Attempt::Settled;
    }
    let revision = publisher.revision_for(&entries);
    let legacy = publisher.legacy_link.is_some() && publisher.legacy_link == state.link;
    let shares: Vec<_> = entries
        .iter()
        .filter(|entry| !legacy || entry.audience != Audience::People)
        .collect();
    let params = json!({
        "revision": revision,
        "shares": shares,
        "visitors": publication.visitors,
    });
    match center.call(scope, PUBLISH_METHOD, params).await {
        Ok(reply) => {
            let Ok(mut outcome) = serde_json::from_value::<Outcome>(reply) else {
                tracing::warn!("Center returned an invalid share publication result");
                return Attempt::Retry;
            };
            if legacy {
                outcome.rejected.extend(
                    entries
                        .iter()
                        .filter(|entry| entry.audience == Audience::People)
                        .map(|entry| Rejected {
                            resource_kind: entry.resource_kind,
                            resource_key: entry.resource_key.clone(),
                            group_id: None,
                            setting_id: entry.setting_id,
                            code: "direct_shares_unsupported".to_owned(),
                        }),
                );
            }
            publisher.sent = Some(Sent {
                link: state.link,
                revision,
                entries,
            });
            book.apply(outcome);
            Attempt::Settled
        }
        // Older centers reject the entire list on a direct entry. Publish the group
        // entries on that link and try direct sharing again on the next logical link.
        Err(CallError::Rejected(code))
            if code == "invalid_request"
                && !legacy
                && state.link.is_some()
                && entries
                    .iter()
                    .any(|entry| entry.audience == Audience::People) =>
        {
            publisher.legacy_link = state.link;
            Attempt::Retry
        }
        // A revision the center already holds with other content: publish anew.
        Err(CallError::Rejected(code)) if code == "conflict" => {
            publisher.sent = None;
            publisher.revision = revision;
            Attempt::Retry
        }
        // The link changed or the center dropped the method; state changes retrigger.
        Err(CallError::Unavailable | CallError::Unsupported) => Attempt::Settled,
        Err(error) => {
            tracing::warn!(%error, "Share publication failed; retrying");
            Attempt::Retry
        }
    }
}

#[cfg(test)]
mod tests;
