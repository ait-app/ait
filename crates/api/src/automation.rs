//! Hub automation on this Host: the Ait center's own sessions, which reach only the
//! Workspace Bots that have an enabled center channel (a Hub or webhook channel).
//!
//! The owner opts in per Bot by enabling such a channel. A center session is
//! admitted while at least one active Bot has one and ends as soon as none does;
//! [`policy`] decides every request it sends.

pub(crate) mod policy;

use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use bot::service::{Bots, HubBot};
use model::ErrorCode;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Admission of the center's automation sessions.
#[derive(Debug, Default)]
pub(crate) struct Automation {
    /// Cancelled when no active Bot has an enabled Hub channel; admitted sessions hold a clone.
    admitted: Mutex<CancellationToken>,
    /// Set once the task that ends sessions on closure is running.
    watching: OnceLock<()>,
}

/// One admitted automation session; it ends when the last Hub channel closes.
#[derive(Debug)]
pub(crate) struct Session {
    cancellation: CancellationToken,
}

impl Session {
    /// Cancelled once no active Bot has an enabled Hub channel.
    pub(crate) const fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }
}

/// Whether the center may reach any Bot.
pub(crate) fn reachable(bots: &[HubBot]) -> bool {
    bots.iter().any(|bot| bot.state == "active")
}

impl Automation {
    /// Admit one automation session while an active Bot has an enabled Hub channel.
    ///
    /// # Errors
    /// Returns `Forbidden` without the Bot service or such a channel.
    pub(crate) fn admit(self: &Arc<Self>, bots: Option<&Bots>) -> Result<Session, ErrorCode> {
        let hub = bots.ok_or(ErrorCode::Forbidden)?.hub();
        // Hold the token lock so a concurrent closure cannot slip between the check and the clone.
        let admitted = lock(&self.admitted);
        if !reachable(&hub.borrow()) {
            return Err(ErrorCode::Forbidden);
        }
        self.watch(hub);
        Ok(Session {
            cancellation: admitted.clone(),
        })
    }

    /// End every admitted session whenever the last reachable Bot closes.
    fn watch(self: &Arc<Self>, mut hub: watch::Receiver<Vec<HubBot>>) {
        if self.watching.set(()).is_err() {
            return;
        }
        let automation = Arc::downgrade(self);
        tokio::spawn(async move {
            while hub.changed().await.is_ok() {
                let open = reachable(&hub.borrow_and_update());
                let Some(automation) = automation.upgrade() else {
                    return;
                };
                if !open {
                    std::mem::take(&mut *lock(&automation.admitted)).cancel();
                }
            }
        });
    }
}

#[cfg(test)]
mod tests;
