//! Native execution and discovery lanes. Blocking registry work stays off HTTP/WS reactors.

use std::fmt;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use metadata::ports::registry::{ProjectRegistry, WorkspaceRegistry};
use metadata::service::session::SessionEvents;
use serde_json::{Value, json};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot};
use tokio_util::sync::CancellationToken;

mod catalog;
mod voice;
pub(crate) mod waits;

use crate::ports::agent_runtime::AgentRuntimeRegistry;
use crate::rpc::ErrorCode;
use crate::rpc::agent_execution::ExecutionState;
use crate::service::agent_manager::AgentManager;
use crate::service::agent_runtime::AgentRuntimeDirectory;

/// Independently composed dependencies retained for the full worker lifetime.
pub struct ExecutionDependencies {
    /// Native session owner with registered clients.
    pub manager: AgentManager,
    /// Agent metadata service using the same registry instance.
    pub directory: AgentRuntimeDirectory,
    /// Shared durable Agent records.
    pub registry: Box<dyn AgentRuntimeRegistry>,
    /// Shared Workspace records for placement validation.
    pub workspaces: Box<dyn WorkspaceRegistry>,
    /// Shared Project records for active placement validation.
    pub projects: Box<dyn ProjectRegistry>,
    /// Optional metadata coordinator for opening imported session Workspaces.
    pub import_directory: Option<metadata::service::directory::Directory>,
    /// Shared setup coordinator, started only after a worktree Agent is registered.
    pub workspace_automation:
        Option<Arc<Mutex<metadata::service::workspace_automation::WorkspaceAutomation>>>,
    /// Host resource guard, such as the data-directory lease.
    pub lifetime: Arc<dyn Send + Sync>,
}

enum Command {
    ObserveWait {
        identifier: String,
        reply: oneshot::Sender<Result<waits::WaitObservation, ErrorCode>>,
    },
    Request {
        method: String,
        params: Value,
        reply: oneshot::Sender<Result<Value, ErrorCode>>,
        cancel: Option<CancellationToken>,
    },
    Shutdown(oneshot::Sender<Result<(), ErrorCode>>),
}

struct Worker {
    sender: mpsc::Sender<Command>,
    catalog: mpsc::Sender<catalog::Request>,
    catalog_shutdown: CancellationToken,
    catalog_responses: Arc<Semaphore>,
    thread: Mutex<Option<JoinHandle<()>>>,
    events: SessionEvents,
    timeline: crate::storage::timeline::Timeline,
    creations: metadata::service::creation::Creations,
    registry: Arc<dyn AgentRuntimeRegistry>,
}

// Field drop order keeps the instance lease until the runtime has reaped its children,
// including unwinding paths before the explicit shutdown handshake.
struct OwnedRuntime {
    runtime: tokio::runtime::Runtime,
    _lifetime: Arc<dyn Send + Sync>,
}

/// Cloneable bounded command handle; native sessions outlive individual client connections.
#[derive(Clone)]
pub struct AgentExecution(Arc<Worker>);

/// Owns one discovery response slot through transport delivery, including after worker completion.
pub(crate) struct CatalogResponse {
    receiver: oneshot::Receiver<Result<Value, ErrorCode>>,
    _permit: OwnedSemaphorePermit,
}

impl CatalogResponse {
    /// Wait for the admitted result without releasing the bounded response slot.
    pub(crate) async fn receive(&mut self) -> Result<Value, ErrorCode> {
        (&mut self.receiver).await.map_err(|_| ErrorCode::AgentIo)?
    }
}

impl fmt::Debug for AgentExecution {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentExecution")
            .finish_non_exhaustive()
    }
}

impl AgentExecution {
    /// Start a dedicated current-thread runtime with independent execution and discovery lanes.
    ///
    /// # Errors
    /// Returns an I/O error if a runtime or worker thread cannot be created.
    pub fn spawn(mut dependencies: ExecutionDependencies) -> Result<Self, std::io::Error> {
        dependencies
            .manager
            .recover_permissions()
            .map_err(std::io::Error::other)?;
        let timeline = match dependencies.manager.timeline() {
            Some(timeline) => timeline,
            None => crate::storage::timeline::Timeline::memory()
                .map_err(|_| std::io::Error::other("initialize timeline"))?,
        };
        timeline
            .recover_inputs()
            .map_err(|_| std::io::Error::other("recover input receipts"))?;
        dependencies.manager = dependencies.manager.with_timeline(timeline.clone());
        dependencies
            .manager
            .recover_titles()
            .map_err(|_| std::io::Error::other("recover Agent titles"))?;
        let creations = dependencies.manager.creations();
        let events = dependencies.manager.events();
        let registry: Arc<dyn AgentRuntimeRegistry> = dependencies.registry.into();
        let worker_registry = registry.clone();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let (sender, receiver) = mpsc::channel(64);
        let (catalog_sender, catalog_receiver) = mpsc::channel(64);
        let catalog_shutdown = CancellationToken::new();
        let worker_catalog_shutdown = catalog_shutdown.clone();
        let (catalog, clients) = dependencies.manager.take_catalog();
        let catalog_events = events.clone();
        let thread = std::thread::Builder::new()
            .name("provider".to_owned())
            .spawn(move || {
                let owned = OwnedRuntime {
                    runtime,
                    _lifetime: dependencies.lifetime,
                };
                let state = ExecutionState {
                    manager: dependencies.manager,
                    directory: dependencies.directory,
                    registry: worker_registry,
                    workspaces: dependencies.workspaces,
                    projects: dependencies.projects,
                    import_directory: dependencies.import_directory,
                    workspace_automation: dependencies.workspace_automation,
                };
                owned.runtime.block_on(async {
                    let execution = async {
                        serve(state, receiver).await;
                        worker_catalog_shutdown.cancel();
                    };
                    tokio::join!(
                        execution,
                        catalog::serve(
                            catalog,
                            clients,
                            catalog_events,
                            catalog_receiver,
                            worker_catalog_shutdown.clone(),
                        ),
                    );
                });
            })?;
        Ok(Self(Arc::new(Worker {
            sender,
            catalog: catalog_sender,
            catalog_shutdown,
            catalog_responses: Arc::new(Semaphore::new(64)),
            thread: Mutex::new(Some(thread)),
            events,
            timeline,
            creations,
            registry,
        })))
    }

    /// Check an exact durable identity without waiting behind native startup or input execution.
    /// Includes archived/internal Agents; callers must perform this blocking read off the reactor.
    /// # Errors
    /// Returns a registry error when the identity cannot be read.
    pub fn contains_identity(&self, id: &str) -> Result<bool, ErrorCode> {
        self.0
            .registry
            .get(id)
            .map(|record| record.is_some())
            .map_err(|_| ErrorCode::AgentIo)
    }

    /// Return the installed durable timeline projection and its observers.
    #[must_use]
    pub fn timeline(&self) -> crate::storage::timeline::Timeline {
        self.0.timeline.clone()
    }

    /// Return shared metadata-owned creation receipts.
    #[must_use]
    pub fn creations(&self) -> metadata::service::creation::Creations {
        self.0.creations.clone()
    }

    /// Return connection events shared with this worker's Agent manager.
    #[must_use]
    pub fn events(&self) -> SessionEvents {
        self.0.events.clone()
    }

    /// Execute a canonical execution or Agent metadata request.
    ///
    /// Catalog discovery uses a separate bounded lane and cache owner. Wait requests poll
    /// without holding execution; caller cancellation does not cancel accepted native work.
    ///
    /// # Errors
    /// Returns safe validation, admission, provider or registry errors.
    pub async fn execute(&self, method: &str, params: Value) -> Result<Value, ErrorCode> {
        if catalog::handles(method) {
            return self.catalog(method, params).await;
        }
        if method != "agent.finish.wait.request" {
            return self.call(method, params).await;
        }
        self.wait(params).await
    }

    /// Drain accepted commands, close native children, and join the owning thread.
    ///
    /// # Errors
    /// Returns a native close or worker failure. The host lifetime guard remains owned until
    /// the worker has actually terminated, including after the calling future is dropped.
    pub async fn shutdown(&self) -> Result<(), ErrorCode> {
        self.0.catalog_shutdown.cancel();
        let (reply, receiver) = oneshot::channel();
        let sent = self.0.sender.send(Command::Shutdown(reply)).await.is_ok();
        let result = if sent {
            receiver.await.unwrap_or(Err(ErrorCode::AgentIo))
        } else {
            Ok(())
        };
        let thread = self.0.thread.lock().map_err(|_| ErrorCode::AgentIo)?.take();
        if let Some(thread) = thread {
            tokio::task::spawn_blocking(move || thread.join())
                .await
                .map_err(|_| ErrorCode::AgentIo)?
                .map_err(|_| ErrorCode::AgentIo)?;
        }
        result
    }

    async fn catalog(&self, method: &str, params: Value) -> Result<Value, ErrorCode> {
        self.admit_catalog(method, params)?.receive().await
    }

    /// Synchronously reserve bounded discovery work before a transport spawns its response task.
    /// The returned owner retains its slot until delivery or caller cancellation.
    /// # Errors
    /// Rejects non-catalog methods, shutdown, a closed worker or exhausted request/response slots.
    pub(crate) fn admit_catalog(
        &self,
        method: &str,
        params: Value,
    ) -> Result<CatalogResponse, ErrorCode> {
        if !catalog::handles(method) {
            return Err(ErrorCode::UnsupportedCapability);
        }
        if self.0.catalog_shutdown.is_cancelled() {
            return Err(ErrorCode::AgentIo);
        }
        let permit = self
            .0
            .catalog_responses
            .clone()
            .try_acquire_owned()
            .map_err(|_| ErrorCode::CatalogBusy)?;
        let (reply, receiver) = oneshot::channel();
        self.0
            .catalog
            .try_send(catalog::Request {
                method: method.to_owned(),
                params,
                reply,
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => ErrorCode::CatalogBusy,
                mpsc::error::TrySendError::Closed(_) => ErrorCode::AgentIo,
            })?;
        Ok(CatalogResponse {
            receiver,
            _permit: permit,
        })
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value, ErrorCode> {
        self.call_cancellable(method, params, None).await
    }

    async fn call_cancellable(
        &self,
        method: &str,
        params: Value,
        cancel: Option<CancellationToken>,
    ) -> Result<Value, ErrorCode> {
        let (reply, receiver) = oneshot::channel();
        self.0
            .sender
            .try_send(Command::Request {
                method: method.to_owned(),
                params,
                reply,
                cancel,
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => ErrorCode::CatalogBusy,
                mpsc::error::TrySendError::Closed(_) => ErrorCode::AgentIo,
            })?;
        receiver.await.map_err(|_| ErrorCode::AgentIo)?
    }
}

async fn serve(mut state: ExecutionState, mut commands: mpsc::Receiver<Command>) {
    let mut interval = tokio::time::interval(Duration::from_millis(25));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            command = commands.recv() => match command {
                Some(Command::Request { method, params, reply, cancel }) => {
                    voice::dispatch(&mut state, &method, params, (reply, cancel)).await;
                }
                Some(Command::ObserveWait { identifier, reply }) => {
                    let _ = reply.send(state.observe_wait(&identifier));
                }
                Some(Command::Shutdown(reply)) => {
                    commands.close();
                    let result = state.manager.close_all().await.map_err(|_| ErrorCode::AgentIo);
                    let _ = reply.send(result);
                    break;
                }
                None => { let _ = state.manager.close_all().await; break; }
            },
            _ = interval.tick() => {
                // Failed durable writes retain their pending event and are retried here.
                let _ = state.manager.poll().await;
                let _ = state.manager.reconcile().await;
                let _ = state.manager.dispatch_pending_inputs().await;
                let _ = state.manager.poll_generated_titles().await;
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests;
