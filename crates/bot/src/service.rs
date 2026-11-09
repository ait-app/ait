//! Bounded, connection-independent Bot runtime and daemon lifecycle.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use domain::bot::{Channel, Conversation, DeliveryStatus, Turn, TurnStatus};
use domain::{BotTurnId, ChannelId, ConversationId};
use model::ErrorCode;
use model::bot::Store;
use model::workspace::registry::{ProjectRegistry, WorkspaceRegistry};
use secrecy::SecretString;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::channels::Incoming;
use crate::channels::feishu::onboarding::Onboarding;
use crate::mcp::Scopes;
use crate::ports::{Credentials, Executor};
use crate::protocol::decode;
use crate::state::{State, StoredChannel};

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
    /// Host credential resolver and private secret storage.
    pub credentials: Arc<dyn Credentials>,
    /// Daemon-wide scoped MCP credential registry, refreshed on every catalog change.
    pub scopes: Arc<Scopes>,
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
    scopes: Arc<Scopes>,
    onboarding: Onboarding,
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
    Finished(ConversationId),
    Rotate(ConversationId, BotTurnId),
    DeliveryNext(ChannelId),
    Delivery(BotTurnId, DeliveryStatus),
    Retire(Vec<String>),
    ChannelAdmission(String),
    CreateStored(StoredChannel, SecretString),
}
/// Outcome of channel input that the Bot did not refuse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Admission {
    /// The message is durably queued (or was already queued under the same id).
    Accepted,
    /// The global turn backlog is full; the message was not stored and must be retried.
    Backlogged,
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
            dependencies.scopes.clone(),
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
            scopes: dependencies.scopes,
            onboarding: Onboarding::default(),
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
        let onboarding = &self.0.onboarding;
        match method {
            "bot.channel.feishu.scan.start.request" => {
                onboarding.start(self, decode(params)?).await
            }
            // Detached so a client disconnecting mid-poll cannot discard an approved scan.
            "bot.channel.feishu.scan.status.request" => {
                let bots = self.clone();
                let request = decode(params)?;
                tokio::spawn(async move { bots.0.onboarding.status(&bots, request).await })
                    .await
                    .map_err(|_| ErrorCode::BotIo)?
            }
            "bot.channel.feishu.scan.cancel.request" => onboarding.cancel(decode(params)?).await,
            "bot.channel.feishu.connect.request" => onboarding.connect(self, decode(params)?).await,
            _ => self.call(Operation::Rpc(method.to_owned(), params)).await,
        }
    }

    /// The scoped MCP credential registry this service publishes authorization into.
    #[must_use]
    pub fn scopes(&self) -> &Arc<Scopes> {
        &self.0.scopes
    }

    /// Stop admission and signal channel receivers and active turns to shut down.
    pub fn stop(&self) {
        self.0.cancellation.cancel();
    }

    /// Retire the Workspaces' Bots and stop their work before archived directories are removed.
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
    /// Admit channel input, waiting for command capacity so a busy service never drops it.
    ///
    /// Returns [`Admission::Backlogged`] when the global turn backlog is full; the channel
    /// keeps the message and retries later.
    /// # Errors
    /// `ResourceExhausted` reports a backlog owned by this message's conversation (or the
    /// conversation catalog limit); `InvalidMessage` and `IdempotencyConflict` report a message
    /// that can never be admitted; other codes report lifecycle or persistence failures.
    pub(crate) async fn accept(&self, incoming: Incoming) -> Result<Admission, ErrorCode> {
        if self.0.cancellation.is_cancelled() {
            return Err(ErrorCode::ServerDraining);
        }
        let receipt = self.send(Operation::Accept(incoming)).await?;
        Ok(if receipt["accepted"] == true {
            Admission::Accepted
        } else {
            Admission::Backlogged
        })
    }
    /// Whether the Workspace's Bot could keep one more channel right now.
    pub(crate) async fn check_channel_admission(&self, workspace: String) -> Result<(), ErrorCode> {
        self.call(Operation::ChannelAdmission(workspace))
            .await
            .map(|_| ())
    }
    /// Durably store a verified secret and enable its channel, waiting for command capacity
    /// because the secret cannot be requested from the external account a second time.
    pub(crate) async fn create_stored_channel(
        &self,
        channel: StoredChannel,
        secret: SecretString,
    ) -> Result<Value, ErrorCode> {
        self.send(Operation::CreateStored(channel, secret)).await
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
    /// Move `turn`'s conversation to a new native session and generation; see `State::rotate`.
    pub(crate) async fn rotate(
        &self,
        conversation: ConversationId,
        turn: BotTurnId,
    ) -> Result<Conversation, ErrorCode> {
        decode(self.send(Operation::Rotate(conversation, turn)).await?)
    }
    pub(crate) async fn finished(&self, conversation: ConversationId) -> Result<(), ErrorCode> {
        self.send(Operation::Finished(conversation))
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
            decode(value).map(Some)
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
        let host_credentials = credentials.clone();
        let processed = tokio::task::spawn_blocking(move || {
            if let Some(command) = command {
                let result = apply(
                    &mut state,
                    &mut accounts,
                    host_credentials.as_ref(),
                    command.operation,
                );
                let _ = command.reply.send(result);
            }
            for channel in std::mem::take(&mut state.released_credentials) {
                if let Err(error) = host_credentials.delete(channel) {
                    tracing::warn!(%channel, ?error, "could not remove a deleted channel's secret");
                }
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
    credentials: &dyn Credentials,
    operation: Operation,
) -> Result<Value, ErrorCode> {
    match operation {
        Operation::Rpc(method, params) if method == "bot.turn.cancel.request" => {
            state.cancel(&decode(params)?)
        }
        Operation::Rpc(method, params) => state.execute(&method, params),
        Operation::Accept(incoming) => state.accept_channel(incoming),
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
        Operation::Finished(conversation) => {
            state.finished(conversation);
            Ok(Value::Null)
        }
        Operation::Rotate(conversation, turn) => state.rotate(conversation, turn),
        Operation::DeliveryNext(channel) => {
            let pending = state.pending_delivery(channel)?;
            if let Some((turn, _)) = &pending {
                state.delivery(turn.id, DeliveryStatus::Sending)?;
            }
            serde_json::to_value(pending).map_err(|_| ErrorCode::BotIo)
        }
        Operation::Delivery(turn, status) => state.delivery(turn, status).map(|()| Value::Null),
        Operation::Retire(workspaces) => state.retire(&workspaces).map(|()| Value::Null),
        Operation::ChannelAdmission(workspace) => state
            .check_channel_admission(&workspace)
            .map(|()| Value::Null),
        Operation::CreateStored(channel, secret) => {
            state.create_stored_channel(channel, &secret, credentials)
        }
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
