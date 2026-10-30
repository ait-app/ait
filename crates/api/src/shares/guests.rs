//! Live guest connections and the resources each visiting account created.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::Shares;

/// Terminal identities remembered per grant; the Host runs far fewer terminals than this.
const MAX_TERMINALS: usize = 128;

/// One visiting account's access through one share.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Grant {
    /// Center share identity.
    pub(crate) share_id: Uuid,
    /// Visiting account.
    pub(crate) user_id: Uuid,
}

/// A grant that lost authorization, with the terminals its guests created.
#[derive(Debug)]
pub(super) struct Revoked {
    pub(super) grant: Grant,
    pub(super) terminals: Vec<String>,
}

#[derive(Debug)]
struct Live {
    grant: Grant,
    cancel: CancellationToken,
}

/// Registry of live guest connections and the grants seen since the daemon started.
#[derive(Debug, Default)]
pub(super) struct Guests {
    next: u64,
    live: BTreeMap<u64, Live>,
    grants: BTreeMap<Grant, VecDeque<String>>,
}

impl Guests {
    fn register(&mut self, grant: Grant) -> (u64, CancellationToken) {
        self.next = self.next.wrapping_add(1);
        let cancel = CancellationToken::new();
        self.live.insert(
            self.next,
            Live {
                grant,
                cancel: cancel.clone(),
            },
        );
        self.grants.entry(grant).or_default();
        (self.next, cancel)
    }

    pub(super) fn unregister(&mut self, id: u64) {
        self.live.remove(&id);
    }

    /// Remember `terminal_id` for `grant`; false when the grant was already revoked.
    pub(super) fn record_terminal(&mut self, grant: Grant, terminal_id: String) -> bool {
        let Some(terminals) = self.grants.get_mut(&grant) else {
            return false;
        };
        if terminals.len() >= MAX_TERMINALS {
            terminals.pop_front();
        }
        terminals.push_back(terminal_id);
        true
    }

    /// End every connection of each grant for which `stale` holds and forget the grant.
    pub(super) fn revoke(&mut self, stale: impl Fn(&Grant) -> bool) -> Vec<Revoked> {
        let revoked: Vec<Grant> = self.grants.keys().copied().filter(&stale).collect();
        for live in self.live.values().filter(|live| stale(&live.grant)) {
            live.cancel.cancel();
        }
        revoked
            .into_iter()
            .map(|grant| Revoked {
                grant,
                terminals: self
                    .grants
                    .remove(&grant)
                    .map(Vec::from)
                    .unwrap_or_default(),
            })
            .collect()
    }

    #[cfg(test)]
    pub(super) fn live(&self) -> usize {
        self.live.len()
    }
}

/// Admission of one guest connection; dropping it unregisters the connection.
#[derive(Debug)]
pub(crate) struct Registration {
    shares: Arc<Shares>,
    id: u64,
    grant: Grant,
    cancel: CancellationToken,
}

impl Registration {
    pub(super) fn new(shares: &Arc<Shares>, guests: &mut Guests, grant: Grant) -> Self {
        let (id, cancel) = guests.register(grant);
        Self {
            shares: shares.clone(),
            id,
            grant,
            cancel,
        }
    }

    /// Cancelled when the share stops admitting this connection.
    pub(crate) fn cancellation(&self) -> &CancellationToken {
        &self.cancel
    }

    /// Remember a terminal this guest created so revocation can end it.
    /// Returns false when the grant was revoked meanwhile; the caller must end the terminal.
    pub(crate) fn record_terminal(&self, terminal_id: String) -> bool {
        self.shares.record_terminal(self.grant, terminal_id)
    }

    /// The share and account this connection was admitted for.
    pub(crate) fn grant(&self) -> Grant {
        self.grant
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.shares.unregister(self.id);
    }
}
