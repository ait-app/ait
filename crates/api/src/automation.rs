//! Hub automation on this Host: the owner's settings and the confinement of
//! the center's automation sessions.
//!
//! The owner changes [`Settings`] over RPC; the daemon publishes them to the
//! center. An automation session is admitted only while they are enabled and
//! ends as soon as they are disabled; [`policy`] decides every request it sends.

pub(crate) mod policy;

use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use model::automation::Settings;
use model::hub::{OutboxStore, StoreError};
use model::methods::MethodSpec;
use model::{Context, DispatchError, ErrorCode, Request};
use serde::Deserialize as _;
use serde_json::{Value, json};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::Shared;

/// Owner methods for the automation settings; automation and guest sessions never reach them.
pub(crate) const METHODS: &[MethodSpec] = &[
    MethodSpec::request("automation.settings.get.request"),
    MethodSpec::request("automation.settings.set.request"),
];

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The Host's automation settings, persisted in `<data_dir>/hub/automation.json`.
#[derive(Debug)]
pub(crate) struct Automation {
    settings: watch::Sender<Settings>,
    store: OnceLock<Arc<dyn OutboxStore>>,
    /// Cancelled when automation is disabled; admitted sessions hold a clone.
    admitted: Mutex<CancellationToken>,
}

impl Default for Automation {
    fn default() -> Self {
        Self {
            settings: watch::Sender::new(Settings::default()),
            store: OnceLock::new(),
            admitted: Mutex::default(),
        }
    }
}

/// One admitted automation session; it ends when automation is disabled.
#[derive(Debug)]
pub(crate) struct Session {
    cancellation: CancellationToken,
}

impl Session {
    /// Cancelled once this Host disables automation.
    pub(crate) const fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }
}

impl Automation {
    /// Load the settings from `store` and persist every later change there.
    ///
    /// # Errors
    /// Returns the store's error for an unreadable or malformed document, or when a
    /// store is already attached.
    pub(crate) fn open(&self, store: Arc<dyn OutboxStore>) -> Result<(), StoreError> {
        let value = store.load()?;
        let settings = if value.is_null() {
            Settings::default()
        } else {
            serde_json::from_value::<Settings>(value)
                .ok()
                .filter(Settings::is_valid)
                .ok_or(StoreError)?
        };
        self.store.set(store).map_err(|_| StoreError)?;
        self.settings.send_replace(settings);
        Ok(())
    }

    /// The current settings.
    pub(crate) fn get(&self) -> Settings {
        self.settings.borrow().clone()
    }

    /// Observe settings changes, for the daemon's publication loop.
    pub(crate) fn subscribe(&self) -> watch::Receiver<Settings> {
        self.settings.subscribe()
    }

    /// Replace the settings, persisting them first. Disabling ends every admitted session.
    ///
    /// # Errors
    /// Returns `InvalidMessage` for settings outside the center's limits and
    /// `RegistryIo` when they cannot be saved.
    pub(crate) fn set(&self, settings: Settings) -> Result<(), ErrorCode> {
        if !settings.is_valid() {
            return Err(ErrorCode::InvalidMessage);
        }
        if let Some(store) = self.store.get() {
            let bytes = serde_json::to_vec(&settings).map_err(|_| ErrorCode::InvalidMessage)?;
            store.save(&bytes).map_err(|_| ErrorCode::RegistryIo)?;
        }
        let enabled = settings.enabled;
        self.settings.send_replace(settings);
        if !enabled {
            std::mem::take(&mut *lock(&self.admitted)).cancel();
        }
        Ok(())
    }

    /// Admit one automation session while automation is enabled.
    ///
    /// # Errors
    /// Returns `Forbidden` while automation is disabled.
    pub(crate) fn admit(&self) -> Result<Session, ErrorCode> {
        // Hold the token lock so a concurrent disable cannot slip between the check and the clone.
        let admitted = lock(&self.admitted);
        if !self.settings.borrow().enabled {
            return Err(ErrorCode::Forbidden);
        }
        Ok(Session {
            cancellation: admitted.clone(),
        })
    }
}

/// Handle an owner settings method, leaving other requests for later handlers.
///
/// # Errors
/// Returns `NotImplemented` for other methods and `Delivery` when the reply cannot be queued.
pub(crate) fn request(
    context: &mut Option<Context<'_>>,
    state: &Shared,
) -> Result<(), DispatchError> {
    let Some(context) = context.take_if(|context| {
        METHODS
            .iter()
            .any(|method| method.name == context.request.method)
    }) else {
        return Err(DispatchError::NotImplemented);
    };
    let result = execute(&context.request, &state.automation);
    context.respond(result).map_err(Into::into)
}

fn execute(request: &Request, automation: &Automation) -> Result<Value, ErrorCode> {
    match request.method.as_str() {
        "automation.settings.get.request" => Ok(json!({ "settings": automation.get() })),
        "automation.settings.set.request" => {
            let settings = Settings::deserialize(&request.params["settings"])
                .map_err(|_| ErrorCode::InvalidMessage)?;
            automation.set(settings)?;
            Ok(json!({ "settings": automation.get() }))
        }
        _ => Err(ErrorCode::MethodNotFound),
    }
}

#[cfg(test)]
mod tests;
