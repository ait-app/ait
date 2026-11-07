//! Coalesced wakeups for directory projections backed by committed state.

use std::sync::{Arc, Mutex};

use tokio::sync::watch;

/// Shared signal for changes that may affect a directory snapshot.
#[derive(Debug, Clone)]
pub struct Changes {
    sender: watch::Sender<()>,
    /// Narrower signals created by [`Changes::extend`] that also observe this one.
    extensions: Arc<Mutex<Vec<Changes>>>,
}

impl Default for Changes {
    fn default() -> Self {
        let (sender, _) = watch::channel(());
        Self {
            sender,
            extensions: Arc::default(),
        }
    }
}

impl Changes {
    /// Wake current observers after the owning service commits a change.
    pub fn notify(&self) {
        self.sender.send_replace(());
        for extension in self
            .extensions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
        {
            extension.notify();
        }
    }

    /// Observe future changes. A caller should compare its initial snapshot once after subscribing.
    #[must_use]
    pub fn subscribe(&self) -> watch::Receiver<()> {
        self.sender.subscribe()
    }

    /// Create a signal whose observers wake for every change to `self` and for changes notified
    /// on the returned signal alone, which observers of `self` never see.
    ///
    /// Facts only one projection reads, such as Workspace checkout facts, notify an extension so
    /// unrelated projections are not recomputed.
    #[must_use]
    pub fn extend(&self) -> Self {
        let extension = Self::default();
        self.extensions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(extension.clone());
        extension
    }
}

#[cfg(test)]
mod tests;
