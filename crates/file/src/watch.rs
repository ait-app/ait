//! Bounded latest-change observations, including atomic replacement and absent files.

use std::fs;
use std::io::ErrorKind;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;

use crate::{Error, File};

/// Latest observed transition of a selected file; events are coalesced rather than queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// An absent or previously inaccessible file is now present.
    Created,
    /// Metadata, permissions, or the backing file identity changed.
    Modified,
    /// The selected path disappeared.
    Removed,
    /// Metadata cannot currently be read; the next successful observation resumes delivery.
    Unavailable(ErrorKind),
}

/// Ownership of one observation task and its bounded latest-change receiver.
#[derive(Debug)]
pub struct Watch {
    receiver: watch::Receiver<Option<Change>>,
    task: JoinHandle<()>,
}

#[derive(Debug, PartialEq, Eq)]
enum Fingerprint {
    Missing,
    Present {
        length: u64,
        modified: Option<SystemTime>,
        readonly: bool,
        #[cfg(unix)]
        identity: (u64, u64, i64, i64),
    },
    Unavailable(ErrorKind),
}

impl Watch {
    pub(crate) async fn start(file: File, interval: Duration) -> Result<Self, Error> {
        if interval.is_zero() {
            return Err(Error::InvalidInterval);
        }
        let runtime =
            tokio::runtime::Handle::try_current().map_err(|_| Error::RuntimeUnavailable)?;
        let file = Arc::new(file);
        let initial_file = file.clone();
        let initial = runtime
            .spawn_blocking(move || fingerprint(&initial_file))
            .await
            .map_err(|_| Error::WatchClosed)?;
        let (sender, receiver) = watch::channel(None);
        let task = runtime.spawn(observe(file, interval, initial, sender));
        Ok(Self { receiver, task })
    }

    /// Wait for a new observation and return its latest transition.
    /// # Errors
    /// Returns `WatchClosed` when the observation task or its Tokio runtime stops.
    pub async fn changed(&mut self) -> Result<Change, Error> {
        self.receiver
            .changed()
            .await
            .map_err(|_| Error::WatchClosed)?;
        (*self.receiver.borrow_and_update()).ok_or(Error::WatchClosed)
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn observe(
    file: Arc<File>,
    period: Duration,
    mut previous: Fingerprint,
    sender: watch::Sender<Option<Change>>,
) {
    let mut interval = tokio::time::interval(period);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            () = sender.closed() => break,
            _ = interval.tick() => {},
        }
        let selected = file.clone();
        let Ok(next) = tokio::task::spawn_blocking(move || fingerprint(&selected)).await else {
            break;
        };
        if next != previous {
            let change = match &next {
                Fingerprint::Missing => Change::Removed,
                Fingerprint::Unavailable(kind) => Change::Unavailable(*kind),
                Fingerprint::Present { .. } if matches!(previous, Fingerprint::Present { .. }) => {
                    Change::Modified
                }
                Fingerprint::Present { .. } => Change::Created,
            };
            sender.send_replace(Some(change));
            previous = next;
        }
    }
}

fn fingerprint(file: &File) -> Fingerprint {
    match fs::metadata(file.path()) {
        Ok(metadata) => Fingerprint::Present {
            length: metadata.len(),
            modified: metadata.modified().ok(),
            readonly: metadata.permissions().readonly(),
            #[cfg(unix)]
            identity: {
                use std::os::unix::fs::MetadataExt;
                (
                    metadata.dev(),
                    metadata.ino(),
                    metadata.ctime(),
                    metadata.ctime_nsec(),
                )
            },
        },
        Err(error) if error.kind() == ErrorKind::NotFound => Fingerprint::Missing,
        Err(error) => Fingerprint::Unavailable(error.kind()),
    }
}

#[cfg(test)]
mod tests;
