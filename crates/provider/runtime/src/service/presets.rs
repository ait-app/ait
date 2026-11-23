//! One preset editor over two stores: a harness-owned one, or Ait's daemon configuration.

pub(crate) mod ait;

use std::sync::Arc;

use model::storage::daemon_config::DaemonConfigStore;

use provider_core::ports::agent_session::AgentClient;
use provider_core::ports::preset::{
    Preset, PresetBackend, PresetDescriptor, PresetEntry, PresetError, PresetSource, editable,
};

/// Largest accepted provider identifier on a preset request.
const MAX_PROVIDER: usize = 128;

/// Run one blocking Ait-store mutation off the reactor while holding its write fence.
///
/// `writes` is a `std::sync::Mutex`, so it must not be held across an `await`; a blocking
/// preset write also commits a temp file with two `fsync` calls, which would stall a Tokio
/// worker for the whole durability window. The guard is therefore acquired and released
/// entirely inside the blocking closure, and only its outcome crosses back to the reactor.
/// A panicking task drops its sender, which reports the store as unavailable rather than
/// hanging the request.
async fn fenced<T: Send + 'static>(
    writes: &Arc<std::sync::Mutex<()>>,
    store: ait::AitPresets,
    operation: impl FnOnce(ait::AitPresets) -> Result<T, ait::AitPresetError> + Send + 'static,
) -> Result<T, PresetError> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let writes = writes.clone();
    tokio::task::spawn_blocking(move || {
        let outcome = writes
            .lock()
            .map_err(|_| "Preset store is unavailable".to_owned())
            .and_then(|_guard| operation(store).map_err(|error| error.to_string()));
        // The receiver may be gone if the caller was cancelled; the write already happened.
        let _ = sender.send(outcome);
    });
    receiver
        .await
        .map_err(|_| PresetError::Unavailable("Preset store is unavailable".to_owned()))?
        .map_err(PresetError::Rejected)
}

/// Run one blocking Ait-store read off the reactor.
///
/// A read takes no write fence: the store's own lock already orders it against a concurrent
/// `patch`, and queueing it behind the fence would let a save delay every list. It reads
/// the same file a save commits, so it still must not run on a Tokio worker.
async fn reading<T: Send + 'static>(
    store: ait::AitPresets,
    operation: impl FnOnce(ait::AitPresets) -> T + Send + 'static,
) -> Result<T, PresetError> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    tokio::task::spawn_blocking(move || {
        // The receiver may be gone if the caller was cancelled; the read has no effect.
        let _ = sender.send(operation(store));
    });
    receiver
        .await
        .map_err(|_| PresetError::Unavailable("Preset store is unavailable".to_owned()))
}

/// Unified preset access for one provider, routed to whichever store owns it.
#[derive(Debug, Clone)]
pub struct PresetService {
    config: Arc<dyn DaemonConfigStore>,
    /// Serializes Ait-store mutations.
    ///
    /// Saving or deleting a preset reads the provider's whole list and writes it back, and the
    /// store's `patch` replaces that array wholesale. Two unserialized writes would each compute
    /// their list from the same snapshot, so the second would drop the first's entry while both
    /// still reported success. Preset requests run on the parallel discovery lane, so the whole
    /// read-modify-write is fenced here instead. One `PresetService` is composed per daemon and
    /// cloned into its consumers, which is what lets this lock span all of them.
    writes: Arc<std::sync::Mutex<()>>,
    revision: Arc<std::sync::atomic::AtomicU64>,
    fence: Arc<tokio::sync::RwLock<()>>,
    #[cfg(test)]
    lookup_started: Arc<tokio::sync::Semaphore>,
    #[cfg(test)]
    lookup_finished: Arc<tokio::sync::Semaphore>,
}

impl PresetService {
    /// Serve Ait-managed presets from `config`.
    pub fn new(config: Arc<dyn DaemonConfigStore>) -> Self {
        Self {
            config,
            writes: Arc::new(std::sync::Mutex::new(())),
            revision: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            fence: Arc::new(tokio::sync::RwLock::new(())),
            #[cfg(test)]
            lookup_started: Arc::new(tokio::sync::Semaphore::new(0)),
            #[cfg(test)]
            lookup_finished: Arc::new(tokio::sync::Semaphore::new(0)),
        }
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn fence(&self) -> Arc<tokio::sync::RwLock<()>> {
        self.fence.clone()
    }

    #[cfg(test)]
    pub(crate) fn signal_lookup_started(&self) {
        self.lookup_started.add_permits(1);
    }
    #[cfg(test)]
    pub(crate) fn signal_lookup_finished(&self) {
        self.lookup_finished.add_permits(1);
    }
    #[cfg(test)]
    pub(crate) async fn wait_lookup_started(&self) {
        self.lookup_started
            .acquire()
            .await
            .expect("lookup signal")
            .forget();
    }
    #[cfg(test)]
    pub(crate) async fn wait_lookup_finished(&self) {
        self.lookup_finished
            .acquire()
            .await
            .expect("lookup signal")
            .forget();
    }

    fn changed(&self) {
        self.revision
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    fn ait(&self) -> ait::AitPresets {
        ait::AitPresets::new(self.config.clone())
    }

    /// Report the store and supported operations for `provider`.
    ///
    /// The descriptor comes from the adapter, so a native store that this harness version
    /// cannot drive is reported read-only instead of failing later on a dead button.
    pub(crate) async fn describe(
        &self,
        cwd: &str,
        client: Option<&Arc<dyn AgentClient>>,
    ) -> PresetDescriptor {
        match client {
            Some(client) => client.preset_descriptor(cwd).await,
            None => PresetDescriptor::unavailable(
                PresetBackend::Ait,
                "This provider is not installed on the host",
            ),
        }
    }

    /// List the presets the editor must show for `provider`.
    ///
    /// A native store answers fully; it is never merged with Ait records, so a provider can
    /// never show two competing lists.
    pub(crate) async fn list(
        &self,
        provider: &str,
        cwd: &str,
        client: Option<&Arc<dyn AgentClient>>,
    ) -> Result<Vec<PresetEntry>, PresetError> {
        if let Some(client) = client
            && client.preset_backend() == PresetBackend::Native
        {
            // A native store reports its private overlay; the editor may only see what a user
            // can actually edit.
            return client.native_presets(cwd).await.map(|presets| {
                presets
                    .into_iter()
                    .map(|mut entry| {
                        entry.preset = editable(entry.preset);
                        entry
                    })
                    .collect()
            });
        }
        reading(self.ait(), {
            let provider = provider.to_owned();
            move |store| store.list(&provider)
        })
        .await?
        .map(|presets| {
            presets
                .into_iter()
                .map(|preset| PresetEntry {
                    preset,
                    source: PresetSource::Ait,
                    editable: true,
                })
                .collect()
        })
        .map_err(|error| PresetError::Unavailable(error.to_string()))
    }

    /// Create or replace one preset through the store that owns it.
    ///
    /// # Errors
    /// Rejects an unusable preset before any store is touched.
    pub(crate) async fn save(
        &self,
        provider: &str,
        cwd: &str,
        client: Option<&Arc<dyn AgentClient>>,
        preset: &Preset,
        original: Option<&str>,
    ) -> Result<(), PresetError> {
        let service = self.clone();
        let provider = provider.to_owned();
        let cwd = cwd.to_owned();
        let client = client.cloned();
        let preset = preset.clone();
        let original = original.map(str::to_owned);
        tokio::spawn(async move {
            let _guard =
                tokio::time::timeout(std::time::Duration::from_secs(60), service.fence.write())
                    .await
                    .map_err(|_| PresetError::Unavailable("Preset store is busy".to_owned()))?;
            service.changed();
            service
                .save_fenced(
                    &provider,
                    &cwd,
                    client.as_ref(),
                    &preset,
                    original.as_deref(),
                )
                .await
        })
        .await
        .map_err(|_| PresetError::Unavailable("Preset store is unavailable".to_owned()))?
    }

    async fn save_fenced(
        &self,
        provider: &str,
        cwd: &str,
        client: Option<&Arc<dyn AgentClient>>,
        preset: &Preset,
        original: Option<&str>,
    ) -> Result<(), PresetError> {
        let preset = &editable(preset.clone());
        preset.validate().map_err(PresetError::Rejected)?;
        if let Some(client) = client
            && client.preset_backend() == PresetBackend::Native
        {
            if original.is_some_and(|original| original != preset.name) {
                return Err(PresetError::Rejected(
                    "This native preset store does not support rename".to_owned(),
                ));
            }
            client.save_native_preset(cwd, preset).await?;
            return Ok(());
        }
        // The lock spans the read-modify-write, not just the write: two concurrent saves of
        // different presets would otherwise both list the same store and the later one would
        // publish a list that drops the other's entry, each still reporting success.
        let provider = provider.to_owned();
        let preset = preset.clone();
        let original = original.map(str::to_owned);
        fenced(&self.writes, self.ait(), move |store| {
            store.save(&provider, &preset, original.as_deref())
        })
        .await?;
        Ok(())
    }

    /// Remove one preset through the store that owns it.
    ///
    /// # Errors
    /// Rejects an unknown name without writing anything.
    pub(crate) async fn delete(
        &self,
        provider: &str,
        cwd: &str,
        client: Option<&Arc<dyn AgentClient>>,
        name: &str,
    ) -> Result<(), PresetError> {
        let service = self.clone();
        let provider = provider.to_owned();
        let cwd = cwd.to_owned();
        let client = client.cloned();
        let name = name.to_owned();
        tokio::spawn(async move {
            let _guard =
                tokio::time::timeout(std::time::Duration::from_secs(60), service.fence.write())
                    .await
                    .map_err(|_| PresetError::Unavailable("Preset store is busy".to_owned()))?;
            service.changed();
            service
                .delete_fenced(&provider, &cwd, client.as_ref(), &name)
                .await
        })
        .await
        .map_err(|_| PresetError::Unavailable("Preset store is unavailable".to_owned()))?
    }

    async fn delete_fenced(
        &self,
        provider: &str,
        cwd: &str,
        client: Option<&Arc<dyn AgentClient>>,
        name: &str,
    ) -> Result<(), PresetError> {
        if !domain::preset::valid_text(name) {
            return Err(PresetError::Rejected(
                "Preset name must be 1-256 characters without control bytes".to_owned(),
            ));
        }
        if let Some(client) = client
            && client.preset_backend() == PresetBackend::Native
        {
            client.delete_native_preset(cwd, name).await?;
            return Ok(());
        }
        // Deleting is the same list-wide rewrite as saving, so it takes the same fence.
        let provider = provider.to_owned();
        let name = name.to_owned();
        fenced(&self.writes, self.ait(), move |store| {
            store.delete(&provider, &name)
        })
        .await?;
        Ok(())
    }
}

/// Whether a request's provider identifier is usable.
pub(crate) fn valid_provider(provider: &str) -> bool {
    !provider.is_empty()
        && provider.len() <= MAX_PROVIDER
        && !provider.chars().any(char::is_control)
}

#[cfg(test)]
mod tests;
