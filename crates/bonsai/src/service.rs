//! The adapter service: its own thread and runtime, started by the host when configured.

use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::config::{Config, is_loopback};
use crate::hello::{self, Offer};
use crate::link::Link;
use crate::outbox::{LastFrame, Outbox};
use crate::ports::Host;
use crate::runs::{Coordinator, Input, WritePolicy};
use crate::store::{Store, StoreError};

/// Time allowed for sessions to persist their last events while stopping.
const DRAIN: Duration = Duration::from_millis(500);

/// Failures starting the service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StartError {
    /// The runtime database could not be opened.
    #[error("open the Bonsai runtime database")]
    Storage,
    /// The worker thread or runtime could not be created.
    #[error("start the Bonsai runtime worker")]
    Worker,
}

impl From<StoreError> for StartError {
    fn from(_: StoreError) -> Self {
        Self::Storage
    }
}

/// Handle to the running adapter.
pub struct Service {
    cancel: CancellationToken,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl std::fmt::Debug for Service {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Service").finish_non_exhaustive()
    }
}

impl Service {
    /// Start the adapter on its own thread.
    ///
    /// # Arguments
    ///
    /// * `config` - Validated connection settings.
    /// * `host` - Host ports.
    /// * `data_dir` - Server data directory; state lives in `bonsai/runtime.sqlite3` below it.
    ///
    /// # Errors
    ///
    /// Returns [`StartError`] when the database or worker cannot be created.
    pub fn spawn(config: Config, host: Host, data_dir: &Path) -> Result<Self, StartError> {
        let store = Store::open(&data_dir.join("bonsai/runtime.sqlite3"))?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| StartError::Worker)?;
        let cancel = CancellationToken::new();
        let stop = cancel.clone();
        let thread = std::thread::Builder::new()
            .name("bonsai".to_owned())
            .spawn(move || runtime.block_on(serve(config, host, store, stop)))
            .map_err(|_| StartError::Worker)?;
        Ok(Self {
            cancel,
            thread: Mutex::new(Some(thread)),
        })
    }

    /// Stop accepting work, close the connection with `1000` and wait for the worker.
    ///
    /// Runs stay `claimed` / `running` at Bonsai, which reconciles on the next connection.
    pub async fn shutdown(&self) {
        self.cancel.cancel();
        let thread = self
            .thread
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(thread) = thread {
            let _ = tokio::task::spawn_blocking(move || thread.join()).await;
        }
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// How agents may write this Bonsai: inject its MCP server when it runs on this machine,
/// otherwise not at all (the owner's own connectors usually reach every space they have).
fn write_policy(config: &Config) -> WritePolicy {
    WritePolicy {
        loopback: is_loopback(config.endpoint()),
    }
}

async fn serve(config: Config, host: Host, store: Store, cancel: CancellationToken) {
    let name = tokio::task::spawn_blocking(hello::machine_name)
        .await
        .unwrap_or_else(|_| "ait".to_owned());
    let offer = Arc::new(RwLock::new(Offer::default()));
    let outbox = Outbox::default();
    let (inputs, input_queue) = mpsc::unbounded_channel();
    let (notices, notice_queue) = mpsc::unbounded_channel();
    let mut coordinator = Coordinator::new(
        host.clone(),
        store.clone(),
        outbox.clone(),
        Arc::clone(&offer),
        notices,
        write_policy(&config),
    );
    // Finish a recovery that is ready; give up on one that hangs once shutdown starts.
    tokio::select! {
        biased;
        () = coordinator.recover() => {}
        () = cancel.cancelled() => return,
    }
    let coordinator = tokio::spawn(coordinator.run(input_queue, notice_queue));
    let link = Link {
        policy: write_policy(&config),
        config,
        host,
        store,
        outbox,
        offer,
        coordinator: inputs.clone(),
        name,
        cancel: cancel.clone(),
        last: Arc::new(Mutex::new(LastFrame::default())),
    };
    link.run().await;
    cancel.cancelled().await;
    let _ = inputs.send(Input::Shutdown);
    let _ = coordinator.await;
    tokio::time::sleep(DRAIN).await;
}

#[cfg(test)]
mod tests;
