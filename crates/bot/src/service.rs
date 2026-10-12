//! Bounded, connection-independent Bot runtime and daemon lifecycle.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use domain::bot::{Channel, Conversation, DeliveryStatus, SessionOwner, Turn, TurnStatus};
use domain::{BotTurnId, ChannelId, ConversationId};
use model::ErrorCode;
use model::bot::Store;
use model::workspace::registry::{ProjectRegistry, WorkspaceRegistry};
use provider::service::bot_execution::Action;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::channels::Incoming;
use crate::ports::{Credentials, Executor};
use crate::state::State;

/// Complete host dependencies; no concrete persistence adapter is required by this crate.
pub struct Dependencies {
    /// Local atomic records and workspace-local memory.
    pub store: Arc<dyn Store>,
    /// Authoritative workspace placement and archive state.
    pub workspaces: Arc<dyn WorkspaceRegistry>,
    /// Authoritative owning project state.
    pub projects: Arc<dyn ProjectRegistry>,
    /// Provider-owned execution.
    pub executor: Arc<dyn Executor>,
    /// Host environment credential resolver.
    pub credentials: Arc<dyn Credentials>,
}

/// Safe startup or worker-lifecycle failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Stored records violate invariants or cannot be recovered.
    #[error("Bot initialization failed")]
    Initialization,
    /// The independent runtime could not be started or joined.
    #[error("Bot worker failed")]
    Worker,
}

#[derive(Debug)]
struct Worker {
    sender: mpsc::Sender<Command>,
    cancellation: CancellationToken,
    closed: watch::Receiver<bool>,
    executor: Arc<dyn Executor>,
}

/// Cloneable application service; conversations outlive UI connections.
#[derive(Debug, Clone)]
pub struct Bots(Arc<Worker>);

enum Operation {
    Rpc(String, Value),
    Accept(Incoming),
    Cursor(ChannelId, i64),
    ChannelStatus(ChannelId, Value),
    Account(ChannelId, String),
    Progress(BotTurnId, TurnStatus, Option<String>, Option<String>),
    Permissions(BotTurnId, Value),
    Finished(ConversationId),
    DeliveryNext(ChannelId),
    Delivery(BotTurnId, DeliveryStatus),
    Permission(crate::protocol::DecisionRequest),
    Retire(Vec<String>),
}
struct Command {
    operation: Operation,
    reply: oneshot::Sender<Result<Value, ErrorCode>>,
}

impl Bots {
    /// Load records, recover interrupted receipts, and start an independent Bot runtime.
    ///
    /// Call from the host's blocking composition path before opening API admission.
    /// # Errors
    /// Returns invalid storage or runtime-start failures; no external account is contacted here.
    pub fn spawn(dependencies: Dependencies) -> Result<Self, Error> {
        let state = State::load(
            dependencies.store,
            dependencies.workspaces,
            dependencies.projects,
        )
        .map_err(|_| Error::Initialization)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| Error::Worker)?;
        let (sender, receiver) = mpsc::channel(64);
        let cancellation = CancellationToken::new();
        let (closed, observed) = watch::channel(false);
        let bots = Self(Arc::new(Worker {
            sender,
            cancellation,
            closed: observed,
            executor: dependencies.executor,
        }));
        let worker = bots.clone();
        std::thread::Builder::new()
            .name("ait-bot".into())
            .spawn(move || {
                runtime.block_on(serve(worker, state, receiver, dependencies.credentials));
                let _ = closed.send(true);
            })
            .map_err(|_| Error::Worker)?;
        Ok(bots)
    }

    /// Execute a canonical Bot RPC; accepted messages return durable receipts immediately.
    /// # Errors
    /// Returns validation, ownership, resource, persistence or lifecycle failures.
    pub async fn execute(&self, method: &str, params: Value) -> Result<Value, ErrorCode> {
        if method == "bot.permission.resolve.request" {
            let request = crate::protocol::decode(params)?;
            let value = self.call(Operation::Permission(request)).await?;
            let owner: SessionOwner = crate::protocol::decode(value["owner"].clone())?;
            let action: Action = crate::protocol::decode(value["action"].clone())?;
            self.0.executor.execute(owner, action).await?;
            return Ok(json!({"resolved":true}));
        }
        self.call(Operation::Rpc(method.to_owned(), params)).await
    }

    /// Stop admission and signal channel receivers and active turns to shut down.
    pub fn stop(&self) {
        self.0.cancellation.cancel();
    }

    /// Disable Bots and stop their owned work before archived workspace directories are removed.
    /// # Errors
    /// Returns configuration persistence or worker lifecycle failures.
    pub async fn retire(&self, workspaces: &[String]) -> Result<(), ErrorCode> {
        self.call(Operation::Retire(workspaces.to_vec()))
            .await
            .map(|_| ())
    }

    /// Wait for all channel tasks and native turn cancellations after stopping admission.
    /// # Errors
    /// Returns worker failure when the completion signal disappears.
    pub async fn shutdown(&self) -> Result<(), Error> {
        self.stop();
        let mut closed = self.0.closed.clone();
        while !*closed.borrow_and_update() {
            closed.changed().await.map_err(|_| Error::Worker)?;
        }
        Ok(())
    }

    async fn call(&self, operation: Operation) -> Result<Value, ErrorCode> {
        if self.0.cancellation.is_cancelled() {
            return Err(ErrorCode::ServerDraining);
        }
        let (reply, result) = oneshot::channel();
        self.0
            .sender
            .try_send(Command { operation, reply })
            .map_err(|_| ErrorCode::ResourceExhausted)?;
        result.await.map_err(|_| ErrorCode::BotIo)?
    }
    async fn send(&self, operation: Operation) -> Result<Value, ErrorCode> {
        let (reply, result) = oneshot::channel();
        self.0
            .sender
            .send(Command { operation, reply })
            .await
            .map_err(|_| ErrorCode::BotIo)?;
        result.await.map_err(|_| ErrorCode::BotIo)?
    }
    pub(crate) async fn accept(&self, incoming: Incoming) -> Result<Value, ErrorCode> {
        self.call(Operation::Accept(incoming)).await
    }
    pub(crate) async fn cursor(&self, channel: ChannelId, cursor: i64) -> Result<(), ErrorCode> {
        self.call(Operation::Cursor(channel, cursor))
            .await
            .map(|_| ())
    }
    pub(crate) async fn channel_status(&self, channel: ChannelId, status: Value) {
        let _ = self.call(Operation::ChannelStatus(channel, status)).await;
    }
    pub(crate) async fn account(
        &self,
        channel: ChannelId,
        identity: String,
    ) -> Result<(), ErrorCode> {
        self.call(Operation::Account(channel, identity))
            .await
            .map(|_| ())
    }
    pub(crate) async fn progress(
        &self,
        turn: BotTurnId,
        status: TurnStatus,
        reply: Option<String>,
        error: Option<String>,
    ) -> Result<(), ErrorCode> {
        self.send(Operation::Progress(turn, status, reply, error))
            .await
            .map(|_| ())
    }
    pub(crate) async fn finished(&self, conversation: ConversationId) -> Result<(), ErrorCode> {
        self.send(Operation::Finished(conversation))
            .await
            .map(|_| ())
    }
    pub(crate) async fn permissions(
        &self,
        turn: BotTurnId,
        permissions: Value,
    ) -> Result<(), ErrorCode> {
        self.send(Operation::Permissions(turn, permissions))
            .await
            .map(|_| ())
    }
    pub(crate) async fn delivery_next(
        &self,
        channel: ChannelId,
    ) -> Result<Option<(Turn, Conversation)>, ErrorCode> {
        let value = self.call(Operation::DeliveryNext(channel)).await?;
        if value.is_null() {
            Ok(None)
        } else {
            crate::protocol::decode(value).map(Some)
        }
    }
    pub(crate) async fn delivery(
        &self,
        turn: BotTurnId,
        status: DeliveryStatus,
    ) -> Result<(), ErrorCode> {
        self.send(Operation::Delivery(turn, status))
            .await
            .map(|_| ())
    }
}

async fn serve(
    bots: Bots,
    mut state: State,
    mut receiver: mpsc::Receiver<Command>,
    credentials: Arc<dyn Credentials>,
) {
    let tasks = TaskTracker::new();
    let mut channels: BTreeMap<ChannelId, (Channel, CancellationToken)> = BTreeMap::new();
    let mut accounts: BTreeMap<String, ChannelId> = BTreeMap::new();
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    let mut stopping = false;
    loop {
        let command = tokio::select! {
            () = bots.0.cancellation.cancelled(), if !stopping => {
                stopping = true;
                for (_, cancel) in channels.values() { cancel.cancel(); }
                for cancel in state.running.values() { cancel.cancel(); }
                None
            },
            command = receiver.recv() => command,
            _ = interval.tick() => None,
        };
        if stopping && state.running.is_empty() {
            break;
        }
        let processed = tokio::task::spawn_blocking(move || {
            if let Some(command) = command {
                let result = apply(&mut state, &mut accounts, command.operation);
                let _ = command.reply.send(result);
            }
            let ready = if stopping {
                Vec::new()
            } else if let Ok(ready) = state.ready() {
                ready
            } else {
                state.frozen = true;
                Vec::new()
            };
            let desired = if stopping || state.frozen {
                Vec::new()
            } else {
                state.channels()
            };
            (state, accounts, ready, desired)
        })
        .await;
        let Ok((next, next_accounts, ready, desired)) = processed else {
            break;
        };
        state = next;
        accounts = next_accounts;
        for run in ready {
            tasks.spawn(crate::execution::run(
                bots.clone(),
                bots.0.executor.clone(),
                run,
            ));
        }
        for (owner, action) in std::mem::take(&mut state.session_actions) {
            let executor = bots.0.executor.clone();
            tasks.spawn(async move {
                let _ = executor.execute(owner, action).await;
            });
        }
        if !stopping {
            refresh_channels(&bots, desired, &credentials, &tasks, &mut channels);
        }
    }
    receiver.close();
    drop(receiver);
    for (_, cancel) in channels.values() {
        cancel.cancel();
    }
    tasks.close();
    tasks.wait().await;
}

fn apply(
    state: &mut State,
    accounts: &mut BTreeMap<String, ChannelId>,
    operation: Operation,
) -> Result<Value, ErrorCode> {
    match operation {
        Operation::Rpc(method, params) if method == "bot.turn.cancel.request" => {
            state.cancel(crate::protocol::decode(params)?)
        }
        Operation::Rpc(method, params) => state.execute(&method, params),
        Operation::Accept(incoming) => state.accept(incoming),
        Operation::Cursor(channel, cursor) => {
            state.advance_cursor(channel, cursor).map(|()| Value::Null)
        }
        Operation::ChannelStatus(channel, value) => {
            state.channel_status.insert(channel, value);
            Ok(Value::Null)
        }
        Operation::Account(channel, identity) => {
            let enabled: BTreeSet<_> = state
                .channels()
                .into_iter()
                .map(|channel| channel.id)
                .collect();
            accounts.retain(|_, id| enabled.contains(id));
            if accounts
                .get(&identity)
                .is_some_and(|owner| *owner != channel)
            {
                return Err(ErrorCode::BotConflict);
            }
            accounts.insert(identity, channel);
            Ok(Value::Null)
        }
        Operation::Progress(turn, status, reply, error) => state
            .progress(turn, status, reply, error)
            .map(|()| Value::Null),
        Operation::Permissions(turn, permissions) => {
            state.permissions(turn, permissions).map(|()| Value::Null)
        }
        Operation::Finished(conversation) => {
            state.finished(conversation);
            Ok(Value::Null)
        }
        Operation::DeliveryNext(channel) => {
            let pending = state.pending_delivery(channel)?;
            if let Some((turn, _)) = &pending {
                state.delivery(turn.id, DeliveryStatus::Sending)?;
            }
            serde_json::to_value(pending).map_err(|_| ErrorCode::BotIo)
        }
        Operation::Delivery(turn, status) => state.delivery(turn, status).map(|()| Value::Null),
        Operation::Permission(request) => {
            let (owner, action) = state.permission(request)?;
            Ok(json!({"owner":owner,"action":action}))
        }
        Operation::Retire(workspaces) => state.retire(&workspaces).map(|()| Value::Null),
    }
}

fn refresh_channels(
    bots: &Bots,
    desired: Vec<Channel>,
    credentials: &Arc<dyn Credentials>,
    tasks: &TaskTracker,
    running: &mut BTreeMap<ChannelId, (Channel, CancellationToken)>,
) {
    running.retain(|id, (channel, cancel)| {
        let keep = desired
            .iter()
            .any(|desired| desired.id == *id && desired.config == channel.config);
        if !keep {
            cancel.cancel();
        }
        keep
    });
    for channel in desired {
        if matches!(channel.config, domain::bot::ChannelConfig::Builtin)
            || running.contains_key(&channel.id)
        {
            continue;
        }
        let cancel = bots.0.cancellation.child_token();
        tasks.spawn(crate::channels::run(
            bots.clone(),
            channel.clone(),
            credentials.clone(),
            cancel.clone(),
        ));
        running.insert(channel.id, (channel, cancel));
    }
}

#[cfg(test)]
mod tests;
