//! Bounded, connection-independent Bot runtime and daemon lifecycle.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use domain::bot::notification::Notification;
use domain::bot::ticket::{TicketStatus, TicketUpdate};
use domain::bot::{Channel, Conversation, DeliveryStatus, Turn, TurnMessage, TurnStatus};
use domain::{BotTurnId, ChannelId, ConversationId, TicketId};
use model::ErrorCode;
use model::bot::Store;
use model::hub::Center;
use model::workspace::registry::{ProjectRegistry, WorkspaceRegistry};
use secrecy::SecretString;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::channels::Incoming;
use crate::channels::feishu::onboarding::Onboarding as FeishuOnboarding;
use crate::channels::photon::onboarding::Onboarding as ImessageOnboarding;
use crate::channels::qq::onboarding::Onboarding as QqOnboarding;
use crate::channels::telegram::onboarding::Onboarding as TelegramOnboarding;
use crate::channels::weixin::onboarding::Onboarding as WeixinOnboarding;
use crate::mcp::Scopes;
use crate::ports::{Credentials, Executor};
use crate::protocol::{ChannelCreate, decode};
use crate::recall::{Origin, Read, Sight};
use crate::state::{State, StoredChannel};
use crate::tickets::{AssignTicket, OpenTicket, OpenedTicket, WorkerReport};

/// One enabled Webhook channel whose endpoint is hosted by the center.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookBot {
    /// Workspace the Bot manages.
    pub workspace_id: String,
    /// The center channel.
    pub channel_id: ChannelId,
    /// Workspace title shown to the account owner.
    pub title: String,
    /// `active` or `paused`.
    pub state: &'static str,
    /// Always `webhook`.
    pub kind: &'static str,
}

/// Outcome of offering one Workspace notification to the Bots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Notified {
    /// Durably logged by at least one active Bot.
    Admitted,
    /// Every active Bot concerned had already logged this notification id.
    Duplicate,
    /// No active Bot is concerned; nothing was stored and no session or model was touched.
    Ignored,
}

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
    /// Cancelled by [`Bots::shutdown`] once offered notifications are in; until then a stopped
    /// worker still logs notifications, such as the results of runs settled while draining.
    closing: CancellationToken,
    /// Notifications [`Bots::offer`] has yet to hand to the worker.
    offers: TaskTracker,
    closed: watch::Receiver<bool>,
    executor: Arc<dyn Executor>,
    scopes: Arc<Scopes>,
    feishu: FeishuOnboarding,
    telegram: TelegramOnboarding,
    weixin: WeixinOnboarding,
    qq: QqOnboarding,
    imessage: ImessageOnboarding,
    /// Enabled center channels of Bots the Ait center may reach.
    webhooks: watch::Sender<Vec<WebhookBot>>,
    /// This Host's serve link, through which webhook endpoints are requested.
    center: OnceLock<Arc<dyn Center>>,
}

/// Cloneable application service; conversations outlive UI connections.
#[derive(Debug, Clone)]
pub struct Bots(Arc<Worker>);

enum Operation {
    Rpc(String, Value),
    Accept(Incoming),
    Cursor(ChannelId, i64),
    ChannelCursor(ChannelId),
    PhotonProjectClaimed(String),
    ChannelStatus(ChannelId, Value),
    Account(ChannelId, String),
    Progress(
        BotTurnId,
        TurnStatus,
        Option<String>,
        Option<Vec<TurnMessage>>,
        Option<String>,
    ),
    Finished(ConversationId),
    Rotate(ConversationId, BotTurnId),
    DeliveryNext(ChannelId),
    Delivery(BotTurnId, DeliveryStatus),
    Retire(Vec<String>),
    ChannelAdmission(String),
    AccountAvailable(String),
    CreateStored(StoredChannel, SecretString),
    SyncToken(ChannelId, String),
    AddSender(ChannelId, String),
    WebhookTurn(String, BotTurnId),
    CreateWebhook(ChannelCreate),
    WebhookChannel(String, ChannelId),
    Notify(Box<Notification>),
    Observe(
        ConversationId,
        String,
        chrono::DateTime<chrono::Utc>,
        std::time::Instant,
    ),
    Ticket(Box<TicketOperation>),
    Read(Box<Read>),
    Sight(String, Origin),
}

/// A ticket entry point of the scoped MCP (ADR-274).
enum TicketOperation {
    Open(OpenTicket),
    Assign(AssignTicket),
    Abandon(String, TicketId),
    Report(WorkerReport),
    Update(String, TicketId, TicketUpdate),
    CloseFinished(String, TicketId),
    Reopen(String, TicketId, String),
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
        let hub = watch::Sender::new(state.webhook_bots());
        let bots = Self(Arc::new(Worker {
            sender,
            cancellation,
            closing: CancellationToken::new(),
            offers: TaskTracker::new(),
            closed: observed,
            executor: dependencies.executor,
            scopes: dependencies.scopes,
            feishu: FeishuOnboarding::default(),
            telegram: TelegramOnboarding::default(),
            weixin: WeixinOnboarding::default(),
            qq: QqOnboarding::default(),
            imessage: ImessageOnboarding::default(),
            webhooks: hub,
            center: OnceLock::new(),
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
        let (feishu, telegram) = (&self.0.feishu, &self.0.telegram);
        let (weixin, qq) = (&self.0.weixin, &self.0.qq);
        match method {
            "bot.channel.feishu.scan.start.request" => feishu.start(self, decode(params)?).await,
            // Status runs detached so a client disconnecting mid-poll cannot lose an approval.
            "bot.channel.feishu.scan.status.request" => {
                let bots = self.clone();
                let request = decode(params)?;
                tokio::spawn(async move { bots.0.feishu.status(&bots, request).await })
                    .await
                    .map_err(|_| ErrorCode::BotIo)?
            }
            "bot.channel.feishu.scan.cancel.request" => feishu.cancel(decode(params)?).await,
            "bot.channel.feishu.connect.request" => {
                feishu
                    .connect(
                        self,
                        decode(params)?,
                        crate::channels::feishu::Platform::Feishu,
                    )
                    .await
            }
            "bot.channel.lark.connect.request" => {
                feishu
                    .connect(
                        self,
                        decode(params)?,
                        crate::channels::feishu::Platform::Lark,
                    )
                    .await
            }
            "bot.channel.telegram.pair.start.request" => {
                telegram.start(self, decode(params)?).await
            }
            "bot.channel.telegram.pair.status.request" => {
                let bots = self.clone();
                let request = decode(params)?;
                tokio::spawn(async move { bots.0.telegram.status(&bots, request).await })
                    .await
                    .map_err(|_| ErrorCode::BotIo)?
            }
            "bot.channel.telegram.pair.cancel.request" => telegram.cancel(decode(params)?).await,
            "bot.channel.imessage.pair.start.request" => {
                self.0.imessage.start(self, decode(params)?).await
            }
            "bot.channel.imessage.pair.status.request" => {
                let bots = self.clone();
                let request = decode(params)?;
                tokio::spawn(async move { bots.0.imessage.status(&bots, request).await })
                    .await
                    .map_err(|_| ErrorCode::BotIo)?
            }
            "bot.channel.imessage.pair.cancel.request" => {
                self.0.imessage.cancel(decode(params)?).await
            }
            "bot.channel.weixin.scan.start.request" => weixin.start(self, decode(params)?).await,
            "bot.channel.weixin.scan.status.request" => {
                let bots = self.clone();
                let request = decode(params)?;
                tokio::spawn(async move { bots.0.weixin.status(&bots, request).await })
                    .await
                    .map_err(|_| ErrorCode::BotIo)?
            }
            "bot.channel.weixin.scan.cancel.request" => weixin.cancel(decode(params)?).await,
            "bot.channel.qq.scan.start.request" => qq.start(self, decode(params)?).await,
            "bot.channel.qq.scan.status.request" => {
                let bots = self.clone();
                let request = decode(params)?;
                tokio::spawn(async move { bots.0.qq.status(&bots, request).await })
                    .await
                    .map_err(|_| ErrorCode::BotIo)?
            }
            "bot.channel.qq.scan.cancel.request" => qq.cancel(decode(params)?).await,
            "bot.channel.qq.connect.request" => qq.connect(self, decode(params)?).await,
            "bot.channel.qq.pair.request" => qq.pair(self, decode(params)?).await,
            "bot.channel.webhook.create.request" => {
                crate::channels::webhook::create(self, decode(params)?).await
            }
            "bot.channel.webhook.rotate.request" => {
                crate::channels::webhook::rotate(self, decode(params)?).await
            }
            _ => self.call(Operation::Rpc(method.to_owned(), params)).await,
        }
    }

    /// Use this Host's serve link for webhook endpoints; later attachments are ignored.
    pub fn attach_center(&self, center: Arc<dyn Center>) {
        let _ = self.0.center.set(center);
    }

    pub(crate) fn center(&self) -> Option<Arc<dyn Center>> {
        self.0.center.get().cloned()
    }

    /// Observe the enabled center channels of Bots the Ait center may reach.
    #[must_use]
    pub fn webhooks(&self) -> watch::Receiver<Vec<WebhookBot>> {
        self.0.webhooks.subscribe()
    }

    /// Refuse a turn that is not in one of the Workspace's enabled center channels.
    /// # Errors
    /// Returns `Forbidden` outside those channels, or worker lifecycle failures.
    pub async fn check_webhook_turn(
        &self,
        workspace: &str,
        turn: BotTurnId,
    ) -> Result<(), ErrorCode> {
        self.call(Operation::WebhookTurn(workspace.to_owned(), turn))
            .await
            .map(|_| ())
    }

    /// Offer one Workspace notification to the active Bots of its Workspace and scope root.
    ///
    /// Returns once the notification is durably logged; it never waits for model execution.
    /// Without an active Bot for either Workspace it returns [`Notified::Ignored`] without
    /// reaching the Bot worker. Wake turns start later, after the coalescing window. After
    /// [`Bots::stop`] notifications are still logged, without waking, until
    /// [`Bots::shutdown`].
    /// # Errors
    /// Returns `InvalidMessage` for malformed identities, `ServerDraining` once shutting down,
    /// and persistence or worker lifecycle failures.
    pub async fn notify(&self, notification: Notification) -> Result<Notified, ErrorCode> {
        let concerned = std::iter::once(notification.workspace_id.as_str())
            .chain(notification.scope.as_deref())
            .any(|root| self.0.scopes.has_active_bot(root));
        if !concerned {
            return Ok(Notified::Ignored);
        }
        if self.0.closing.is_cancelled() {
            return Err(ErrorCode::ServerDraining);
        }
        decode(self.send(Operation::Notify(Box::new(notification))).await?)
    }

    /// Offer `notification` on `runtime` without waiting, for callers that cannot await, such
    /// as the schedule worker settling a run. [`Bots::shutdown`] waits for every offer, so a
    /// result settled while the daemon drains is still logged.
    pub fn offer(&self, runtime: &tokio::runtime::Handle, notification: Notification) {
        let bots = self.clone();
        runtime.spawn(self.0.offers.track_future(async move {
            match bots.notify(notification).await {
                Ok(_) | Err(ErrorCode::ServerDraining) => {}
                Err(error) => tracing::warn!(?error, "Bot notification was not admitted"),
            }
        }));
    }

    /// Record that a turn of `conversation` read Agent `agent` no longer running, in the state
    /// the Agent last changed at `at`, so the conversation is not woken again for what it saw.
    /// `read_started` is when the reading call began; it binds the read to the turn running
    /// then, so a late result of an earlier, cancelled turn's call is ignored.
    ///
    /// Best effort: the observation only suppresses a repeated report and is kept in memory.
    /// # Errors
    /// Returns `ServerDraining` while stopping and worker lifecycle failures.
    pub async fn observe(
        &self,
        conversation: ConversationId,
        agent: String,
        at: chrono::DateTime<chrono::Utc>,
        read_started: std::time::Instant,
    ) -> Result<(), ErrorCode> {
        self.call(Operation::Observe(conversation, agent, at, read_started))
            .await
            .map(|_| ())
    }

    /// Open a ticket for the first Agent a Bot session starts for a task, before the Agent is
    /// created; an earlier attempt with the same key returns its ticket with `created: false`.
    /// # Errors
    /// Returns `ResourceExhausted` at the open ticket limit (report
    /// [`crate::tickets::OPEN_LIMIT_MESSAGE`]), `IdempotencyConflict` when the key opened a
    /// different ticket, `InvalidMessage` for a blank title, a malformed key or a conversation
    /// of another Bot, `BotNotFound` without a Bot, `BotConflict` when it is not active, and
    /// persistence or lifecycle failures.
    pub async fn open_ticket(&self, request: OpenTicket) -> Result<OpenedTicket, ErrorCode> {
        decode(self.ticket(TicketOperation::Open(request)).await?)
    }

    /// Record that an Agent works on a ticket, after the Agent was created; assigning the same
    /// Agent again changes nothing. Any ticket status is accepted.
    /// # Errors
    /// Returns `ResourceExhausted` beyond the assignment limit, `InvalidMessage` for malformed
    /// identities, `BotNotFound` for a ticket of another Bot, `BotConflict` when the Bot is not
    /// active, and persistence or lifecycle failures.
    pub async fn assign_ticket(&self, request: AssignTicket) -> Result<(), ErrorCode> {
        self.ticket(TicketOperation::Assign(request))
            .await
            .map(|_| ())
    }

    /// Delete a ticket [`Bots::open_ticket`] opened whose first Agent could not be created; a
    /// ticket that already has an assignment is kept.
    /// # Errors
    /// Returns `BotNotFound` for a ticket of another Bot, `BotConflict` when the Bot is not
    /// active, and persistence or lifecycle failures.
    pub async fn abandon_ticket(&self, root: String, ticket: TicketId) -> Result<(), ErrorCode> {
        self.ticket(TicketOperation::Abandon(root, ticket))
            .await
            .map(|_| ())
    }

    /// Record what an Agent whose `ait.bot.ticket` label names the ticket reports about it,
    /// assigning the Agent first when it is not assigned and the ticket has room. `blocked`,
    /// `done` and `failed` also wake the Bot in the ticket's conversation through the
    /// notification inbox; `progress` never wakes it. Returns the ticket's status afterwards.
    /// # Errors
    /// Returns `BotConflict` for a closed ticket or a Bot that is not active, `InvalidMessage`
    /// for malformed identities, `BotNotFound` for a ticket of another Bot, and persistence or
    /// lifecycle failures.
    pub async fn report_progress(&self, report: WorkerReport) -> Result<TicketStatus, ErrorCode> {
        decode(self.ticket(TicketOperation::Report(report)).await?)
    }

    /// Apply the Bot's own `update` to its ticket: close, reopen, end a wait, retitle or add a
    /// note. Returns `{"ticket":…,"changed":bool}` like `bot.ticket.update.request`.
    /// # Errors
    /// Returns `InvalidMessage` for an empty or malformed update, `BotConflict` for closing a
    /// closed ticket with another status or a Bot that is not active, `ResourceExhausted` for
    /// reopening at the open ticket limit, `BotNotFound` for a ticket of another Bot, and
    /// persistence or lifecycle failures.
    pub async fn update_ticket(
        &self,
        root: String,
        ticket: TicketId,
        update: TicketUpdate,
    ) -> Result<Value, ErrorCode> {
        self.ticket(TicketOperation::Update(root, ticket, update))
            .await
    }

    /// Close a ticket of the Bot of `root` on Ait's behalf because every Agent working on it
    /// is archived, which the caller judged from a complete read of the Agent directory: as
    /// `done` when its newest worker report says the work finished, otherwise `cancelled`.
    /// Returns the new status, or `None` when the ticket is waiting, closed or has no Agent.
    /// # Errors
    /// Returns `BotNotFound` for a ticket of another Bot, `BotConflict` when the Bot is not
    /// active, and persistence or lifecycle failures.
    pub async fn close_finished_ticket(
        &self,
        root: String,
        ticket: TicketId,
    ) -> Result<Option<TicketStatus>, ErrorCode> {
        decode(
            self.ticket(TicketOperation::CloseFinished(root, ticket))
                .await?,
        )
    }

    /// Reopen a closed ticket of the Bot of `root` on Ait's behalf because the Bot gave
    /// `agent`, one of its Agents, more work; `false` when it was not closed.
    /// # Errors
    /// Returns `ResourceExhausted` at the open ticket limit, `BotNotFound` for a ticket of
    /// another Bot, `BotConflict` when the Bot is not active, and persistence or lifecycle
    /// failures.
    pub async fn reopen_ticket(
        &self,
        root: String,
        ticket: TicketId,
        agent: String,
    ) -> Result<bool, ErrorCode> {
        decode(
            self.ticket(TicketOperation::Reopen(root, ticket, agent))
                .await?,
        )
    }

    /// Read one Bot resource for a scoped MCP caller (ADR-280): tickets, conversations, turns,
    /// notifications and Agents by id, in bounded pages whose cut text points at the resource
    /// that returns it whole. Reads never write, wake or move a conversation's unseen cursor.
    /// # Errors
    /// Returns `InvalidMessage` for an unknown resource or page bounds, `BotNotFound` without a
    /// current Bot or for a missing record, `Forbidden` for a record of a conversation the
    /// reader may not read, `ServerDraining` once stopping, and storage or lifecycle failures.
    pub async fn read(&self, read: Read) -> Result<Value, ErrorCode> {
        if self.0.cancellation.is_cancelled() {
            return Err(ErrorCode::ServerDraining);
        }
        self.send(Operation::Read(Box::new(read))).await
    }

    /// What a caller of `root`'s current Bot whose work came from `origin` may read and reach
    /// (ADR-280 §4): the sight of that conversation, which for the owner conversation is
    /// everything and for another conversation only what it originated and the Agents of its
    /// own tasks. An Agent's work came from its ticket's conversation, or else from its
    /// conversation label. An external viewer comes with the origin of every ticket, so Agents
    /// are decided by their labels without asking again.
    /// # Returns
    /// `None` for an Agent whose work came from no conversation, which is not restricted.
    /// # Errors
    /// Returns `BotNotFound` without a current Bot, `ServerDraining` once stopping, and worker
    /// lifecycle failures.
    pub async fn sight(&self, root: String, origin: Origin) -> Result<Option<Sight>, ErrorCode> {
        if self.0.cancellation.is_cancelled() {
            return Err(ErrorCode::ServerDraining);
        }
        decode(self.send(Operation::Sight(root, origin)).await?)
    }

    /// Run a ticket entry point, waiting for command capacity so a busy worker never refuses
    /// it with the `ResourceExhausted` reserved for the limits.
    async fn ticket(&self, operation: TicketOperation) -> Result<Value, ErrorCode> {
        if self.0.cancellation.is_cancelled() {
            return Err(ErrorCode::ServerDraining);
        }
        self.send(Operation::Ticket(Box::new(operation))).await
    }

    /// The scoped MCP credential registry this service publishes authorization into.
    #[must_use]
    pub fn scopes(&self) -> &Arc<Scopes> {
        &self.0.scopes
    }

    /// Resolve once the service stops admitting work, so host feeders can end with it.
    pub async fn stopped(&self) {
        self.0.cancellation.cancelled().await;
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

    /// Wait for all channel tasks and native turn cancellations after stopping admission, and
    /// for the notifications offered until now.
    /// # Errors
    /// Returns worker failure when the completion signal disappears.
    pub async fn shutdown(&self) -> Result<(), Error> {
        self.stop();
        self.0.offers.close();
        self.0.offers.wait().await;
        self.0.closing.cancel();
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
    /// Whether no enabled channel is already connected to the external account `identity`.
    pub(crate) async fn account_available(&self, identity: String) -> Result<bool, ErrorCode> {
        let available = self.call(Operation::AccountAvailable(identity)).await?;
        Ok(available == true)
    }
    /// Durably store a verified secret and enable its channel, waiting for command capacity
    /// because the secret cannot be requested from the external account a second time.
    /// Save an enabled channel for an endpoint the center has issued.
    pub(crate) async fn create_webhook_channel(
        &self,
        request: ChannelCreate,
    ) -> Result<Value, ErrorCode> {
        self.send(Operation::CreateWebhook(request)).await
    }
    /// Refuse unless `channel` is a live webhook channel of the Workspace's Bot.
    pub(crate) async fn check_webhook_channel(
        &self,
        workspace: String,
        channel: ChannelId,
    ) -> Result<(), ErrorCode> {
        self.call(Operation::WebhookChannel(workspace, channel))
            .await
            .map(|_| ())
    }
    pub(crate) async fn create_stored_channel(
        &self,
        channel: StoredChannel,
        secret: SecretString,
    ) -> Result<Value, ErrorCode> {
        self.send(Operation::CreateStored(channel, secret)).await
    }
    /// Record a channel's stream position after the batch it covers has been queued.
    pub(crate) async fn sync_token(
        &self,
        channel: ChannelId,
        token: String,
    ) -> Result<(), ErrorCode> {
        self.call(Operation::SyncToken(channel, token))
            .await
            .map(|_| ())
    }
    /// Allow a QQ sender who presented the channel's pairing code.
    pub(crate) async fn add_allowed_sender(
        &self,
        channel: ChannelId,
        sender: String,
    ) -> Result<(), ErrorCode> {
        self.send(Operation::AddSender(channel, sender))
            .await
            .map(|_| ())
    }
    /// A channel's status, refusing a channel the requesting Workspace's Bot does not own.
    pub(crate) async fn channel_status_for(&self, params: Value) -> Result<Value, ErrorCode> {
        self.call(Operation::Rpc(
            "bot.channel.status.request".to_owned(),
            params,
        ))
        .await
    }
    /// Consume a QQ channel's pairing code if `text` presents it.
    pub(crate) fn claim_qq_pairing(&self, channel: ChannelId, text: &str) -> bool {
        self.0.qq.claim(channel, text)
    }
    pub(crate) async fn cursor(&self, channel: ChannelId, cursor: i64) -> Result<(), ErrorCode> {
        self.call(Operation::Cursor(channel, cursor))
            .await
            .map(|_| ())
    }
    /// Whether a channel not deleted already uses this Photon project.
    pub(crate) async fn photon_project_claimed(&self, project: String) -> Result<bool, ErrorCode> {
        let claimed = self.call(Operation::PhotonProjectClaimed(project)).await?;
        Ok(claimed == true)
    }
    /// The channel's durably saved stream position, for resuming after a reconnect.
    pub(crate) async fn channel_cursor(
        &self,
        channel: ChannelId,
    ) -> Result<Option<i64>, ErrorCode> {
        let cursor = self.call(Operation::ChannelCursor(channel)).await?;
        Ok(cursor.as_i64())
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
        self.send(Operation::Progress(turn, status, reply, None, error))
            .await
            .map(|_| ())
    }
    pub(crate) async fn progress_with_messages(
        &self,
        turn: BotTurnId,
        status: TurnStatus,
        reply: Option<String>,
        messages: Vec<TurnMessage>,
        error: Option<String>,
    ) -> Result<(), ErrorCode> {
        self.send(Operation::Progress(
            turn,
            status,
            reply,
            Some(messages),
            error,
        ))
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
    let mut closing = false;
    loop {
        let command = tokio::select! {
            () = bots.0.cancellation.cancelled(), if !stopping => {
                stopping = true;
                for (_, cancel) in channels.values() { cancel.cancel(); }
                for cancel in state.running.values() { cancel.cancel(); }
                None
            },
            () = bots.0.closing.cancelled(), if stopping && !closing => {
                closing = true;
                None
            },
            command = receiver.recv() => command,
            _ = interval.tick() => None,
        };
        if closing && state.running.is_empty() {
            break;
        }
        let host_credentials = credentials.clone();
        let hub = bots.clone();
        let processed = tokio::task::spawn_blocking(move || {
            state.stopping = stopping;
            if let Some(command) = command {
                let result = apply(
                    &mut state,
                    &mut accounts,
                    host_credentials.as_ref(),
                    command.operation,
                );
                // Before replying, so a caller sees the Webhook channels its change produced.
                publish_webhooks(&hub, &state, stopping);
                let _ = command.reply.send(result);
            }
            publish_webhooks(&hub, &state, stopping);
            for channel in std::mem::take(&mut state.released_credentials) {
                if let Err(error) = host_credentials.delete(channel) {
                    tracing::warn!(%channel, ?error, "could not remove a deleted channel's secret");
                }
            }
            if !stopping {
                state.wake(chrono::Utc::now());
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
        for channel in std::mem::take(&mut state.released_webhooks) {
            tasks.spawn(crate::channels::webhook::release(bots.clone(), channel));
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
        Operation::SyncToken(channel, token) => {
            state.advance_sync(channel, token).map(|()| Value::Null)
        }
        Operation::AddSender(channel, sender) => state
            .add_allowed_sender(channel, sender)
            .map(|()| Value::Null),
        Operation::Cursor(channel, cursor) => {
            state.advance_cursor(channel, cursor).map(|()| Value::Null)
        }
        Operation::ChannelCursor(channel) => state.channel_cursor(channel).map(Value::from),
        Operation::PhotonProjectClaimed(project) => {
            Ok(Value::Bool(state.photon_project_claimed(&project)))
        }
        Operation::ChannelStatus(channel, value) => {
            state.channel_status.insert(channel, value);
            Ok(Value::Null)
        }
        Operation::AccountAvailable(identity) => {
            retain_enabled_accounts(state, accounts);
            Ok(Value::Bool(!accounts.contains_key(&identity)))
        }
        Operation::Account(channel, identity) => {
            retain_enabled_accounts(state, accounts);
            if accounts
                .get(&identity)
                .is_some_and(|owner| *owner != channel)
            {
                return Err(ErrorCode::BotConflict);
            }
            accounts.insert(identity, channel);
            Ok(Value::Null)
        }
        Operation::Progress(turn, status, reply, messages, error) => state
            .progress_with_messages(turn, status, reply, messages, error)
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
        Operation::WebhookTurn(workspace, turn) => {
            state.webhook_turn(&workspace, turn).map(|()| Value::Null)
        }
        Operation::CreateWebhook(request) => state.create_webhook_channel(request),
        Operation::WebhookChannel(workspace, channel) => state
            .check_webhook_channel(&workspace, channel)
            .map(|()| Value::Null),
        Operation::Observe(conversation, agent, at, read_started) => {
            state.observe(conversation, &agent, at, read_started);
            Ok(Value::Null)
        }
        Operation::Notify(notification) => {
            let notified = state.notify(*notification, chrono::Utc::now())?;
            serde_json::to_value(notified).map_err(|_| ErrorCode::BotIo)
        }
        Operation::Ticket(operation) => apply_ticket(state, *operation),
        Operation::Read(read) => state.read(&read),
        Operation::Sight(root, origin) => {
            serde_json::to_value(state.sight(&root, origin)?).map_err(|_| ErrorCode::BotIo)
        }
    }
}

fn apply_ticket(state: &mut State, operation: TicketOperation) -> Result<Value, ErrorCode> {
    let now = chrono::Utc::now();
    match operation {
        TicketOperation::Open(request) => {
            serde_json::to_value(state.open_ticket(request, now)?).map_err(|_| ErrorCode::BotIo)
        }
        TicketOperation::Assign(request) => {
            state.assign_ticket(&request, now).map(|()| Value::Null)
        }
        TicketOperation::Abandon(root, ticket) => {
            state.abandon_ticket(&root, ticket).map(|()| Value::Null)
        }
        TicketOperation::Report(report) => {
            serde_json::to_value(state.report_progress(&report, now)?).map_err(|_| ErrorCode::BotIo)
        }
        TicketOperation::Update(root, ticket, update) => {
            state.update_ticket(&root, ticket, &update, now)
        }
        TicketOperation::CloseFinished(root, ticket) => {
            serde_json::to_value(state.close_finished_ticket(&root, ticket, now)?)
                .map_err(|_| ErrorCode::BotIo)
        }
        TicketOperation::Reopen(root, ticket, agent) => state
            .reopen_ticket(&root, ticket, &agent, now)
            .map(Value::Bool),
    }
}

/// Publish the Bots the center may reach, if they changed; none while stopping or frozen.
fn publish_webhooks(bots: &Bots, state: &State, stopping: bool) {
    let hub = if stopping || state.frozen {
        Vec::new()
    } else {
        state.webhook_bots()
    };
    bots.0.webhooks.send_if_modified(|current| {
        let changed = *current != hub;
        if changed {
            *current = hub;
        }
        changed
    });
}

fn retain_enabled_accounts(state: &State, accounts: &mut BTreeMap<String, ChannelId>) {
    let enabled: BTreeSet<_> = state
        .channels()
        .into_iter()
        .map(|channel| channel.id)
        .collect();
    accounts.retain(|_, id| enabled.contains(id));
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
        // The center pulls a Webhook channel's replies; no transport runs for it.
        if matches!(
            channel.config,
            domain::bot::ChannelConfig::Builtin | domain::bot::ChannelConfig::Webhook { .. }
        ) || running.contains_key(&channel.id)
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
