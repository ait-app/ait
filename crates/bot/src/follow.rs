//! Tickets that follow pull requests (ADR-287): the ports through which the daemon reads pull
//! requests and reaches a ticket's Agents, the refusals of a binding, and the follower task
//! that polls bound pull requests and delivers what they ask of the ticket's workers.
//!
//! The follower only reads and delivers; what a read means for the ticket, who receives it and
//! whether the Bot wakes is decided by the Bot worker in `State::observe_pull`, so another
//! source of pull request reads, such as webhook deliveries, can feed the same path.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use domain::TicketId;
use domain::bot::follow::{Delivery, Lookups, PullReference, wrote};
use domain::forge::PullSnapshot;
use model::ErrorCode;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use tokio::time::Instant;

use crate::service::Bots;

/// Time between two reads of an open pull request.
pub(crate) const POLL_INTERVAL: Duration = Duration::from_secs(60);
/// Time between two reads of the pull requests of a ticket whose pull requests all ended.
pub(crate) const ENDED_INTERVAL: Duration = Duration::from_mins(10);
/// Longest time between two reads after failures; failures double the interval up to it.
pub(crate) const BACKOFF_LIMIT: Duration = Duration::from_mins(15);
/// Most pull requests one Bot follows at once, across its tickets: the per-Workspace cap.
pub(crate) const FOLLOW_LIMIT: usize = 16;
/// Time between two attempts to deliver inputs that wait for a busy or unreachable Agent.
pub(crate) const PENDING_INTERVAL: Duration = Duration::from_secs(15);
/// Most forge reads the follower makes per [`READ_WINDOW`] for every Bot of this daemon
/// together, so followed pull requests leave the forge account's API budget to its owner.
pub(crate) const READ_BUDGET: usize = 30;
/// Window of [`READ_BUDGET`].
pub(crate) const READ_WINDOW: Duration = Duration::from_secs(60);

/// Why a pull request could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PullReadError {
    /// The repository has no such pull request.
    #[error("no such pull request")]
    NotFound,
    /// The host cannot use a forge here: no CLI, not signed in, or no supported remote.
    #[error("forge unavailable: {0}")]
    Unavailable(String),
    /// Any other failure, such as a network error.
    #[error("{0}")]
    Failed(String),
}

/// Reads pull requests in a Workspace's checkout.
#[async_trait::async_trait]
pub trait PullSource: Send + Sync {
    /// Read pull request `number` of the repository checked out by Workspace `workspace`.
    /// Implementations run the forge CLI on a blocking thread.
    ///
    /// # Errors
    /// Returns [`PullReadError`] when the pull request cannot be read.
    async fn read(&self, workspace: &str, number: u64) -> Result<PullSnapshot, PullReadError>;

    /// Whether `login` can write to the repository checked out by Workspace `workspace`: its
    /// permission there is admin, maintain or write. Only GitHub reads report authors whose
    /// write access needs this lookup (organization members and collaborators).
    ///
    /// `false` also when the forge refuses to tell for good, such as GitHub answering HTTP 403
    /// to a host account without push access: Ait then cannot confirm the author can write.
    ///
    /// # Errors
    /// Returns [`PullReadError`] when the permission cannot be read now; the binding holds the
    /// author's entries back and asks again on a later read, a bounded number of times.
    async fn can_write(&self, workspace: &str, login: &str) -> Result<bool, PullReadError>;
}

/// Reaches the Agents of a ticket.
#[async_trait::async_trait]
pub trait Workers: Send + Sync {
    /// Unarchived Agents whose `ait.bot.ticket` label names `ticket`.
    ///
    /// # Errors
    /// Returns Agent directory failures; the caller then decides nothing.
    async fn live(&self, ticket: TicketId) -> Result<Vec<String>, ErrorCode>;

    /// What the tool calls among the newest timeline entries of `agents` sent (their
    /// arguments, never user messages, delivered inputs or tool output), to recognize what
    /// they wrote on a pull request.
    async fn inputs(&self, agents: &[String]) -> Vec<String>;

    /// Send `text` to `agent` under `message_id` as a new turn, only while the Agent is idle;
    /// delivering the same `message_id` again is one input.
    ///
    /// # Errors
    /// Returns the failure when the Agent refused or could not take the input.
    async fn deliver(
        &self,
        agent: &str,
        message_id: &str,
        text: &str,
    ) -> Result<Delivered, ErrorCode>;
}

/// What became of an input [`Workers::deliver`] was asked to send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivered {
    /// The idle Agent took it as a new turn.
    Started,
    /// The Agent is running a turn; the input waits and is sent once it is idle.
    Busy,
}

/// The ports the daemon installs with [`Bots::attach_follow`].
pub(crate) struct Ports {
    pub(crate) pulls: Arc<dyn PullSource>,
    pub(crate) workers: Arc<dyn Workers>,
    /// One lock per ticket, held while its inputs are sent and their outcome recorded, so two
    /// deliveries of the same ticket input, such as the follower's and an answer's, never
    /// overlap; deliveries of different tickets do not wait for each other.
    pub(crate) delivering: std::sync::Mutex<BTreeMap<TicketId, Arc<tokio::sync::Mutex<()>>>>,
}

impl Ports {
    /// The lock that serializes the deliveries of `ticket`. Locks that no delivery holds or
    /// waits for any more are dropped, so the map only keeps tickets being delivered.
    fn delivery_lock(&self, ticket: TicketId) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self
            .delivering
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        locks.retain(|_, lock| Arc::strong_count(lock) > 1);
        locks.entry(ticket).or_default().clone()
    }
}

impl std::fmt::Debug for Ports {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Ports")
    }
}

/// Why a pull request was not bound; every variant tells the caller what to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PullRefusal {
    /// The reference is neither a number nor a pull request URL.
    #[error("unparsable pull request reference")]
    Reference,
    /// This host cannot read pull requests.
    #[error("pull requests cannot be read on this host")]
    Unsupported,
    /// The forge CLI is missing or signed out, or the checkout has no forge remote.
    #[error("forge unavailable")]
    Unavailable,
    /// The repository has no such pull request.
    #[error("no such pull request")]
    NotFound,
    /// The URL names a pull request of another repository.
    #[error("pull request of another repository")]
    OtherRepository,
    /// The read failed for another reason.
    #[error("pull request read failed")]
    Failed,
}

impl PullRefusal {
    /// Guidance for the caller that tried to bind the pull request.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::Reference => {
                "pullRequest must be a pull request number such as 12 or #12, or its URL; the \
                 report was not recorded"
            }
            Self::Unsupported => {
                "This host cannot follow pull requests; report without pullRequest"
            }
            Self::Unavailable => {
                "Ait could not read pull requests in this Workspace: the gh (GitHub) or glab \
                 (GitLab) CLI must be installed and signed in on this host (check gh auth \
                 status or glab auth status) and the checkout needs an origin remote on that \
                 forge. The report was not recorded; report without pullRequest, or tell the \
                 user what to set up"
            }
            Self::NotFound => {
                "This Workspace's repository has no such pull request; check the number. The \
                 report was not recorded"
            }
            Self::OtherRepository => {
                "That pull request belongs to another repository; only pull requests of this \
                 Workspace's repository can be followed. The report was not recorded"
            }
            Self::Failed => {
                "Ait could not read that pull request right now; try again later or report \
                 without pullRequest. The report was not recorded"
            }
        }
    }
}

/// A ticket the follower serves, as the Bot worker publishes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FollowTarget {
    /// Root Workspace of the ticket's Bot, whose checkout holds the repository.
    pub(crate) root: String,
    /// Ticket.
    pub(crate) ticket: TicketId,
    /// Pull requests the ticket follows now.
    pub(crate) pulls: Vec<FollowedPull>,
    /// Whether every pull request it follows ended, so they are read less often.
    pub(crate) ended: bool,
    /// Whether inputs wait for delivery.
    pub(crate) pending: bool,
}

/// One followed pull request of a [`FollowTarget`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FollowedPull {
    /// Number.
    pub(crate) number: u64,
    /// When the newest entry the binding handled was written; only newer ones are unhandled.
    pub(crate) mark: Option<DateTime<Utc>>,
    /// Failed reads in a row.
    pub(crate) failures: u32,
}

/// One read of a followed pull request, handed to the Bot worker.
#[derive(Debug, Clone)]
pub(crate) struct PullRead {
    pub(crate) root: String,
    pub(crate) ticket: TicketId,
    pub(crate) snapshot: PullSnapshot,
    pub(crate) authored: BTreeSet<String>,
    pub(crate) lookups: Lookups,
    pub(crate) live: Vec<String>,
}

impl Bots {
    /// Install the ports through which this service follows pull requests and reaches
    /// tickets' Agents, and start the follower on `runtime`; later attachments are ignored.
    pub fn attach_follow(
        &self,
        runtime: &tokio::runtime::Handle,
        pulls: Arc<dyn PullSource>,
        workers: Arc<dyn Workers>,
    ) {
        if self.set_follow(Ports {
            pulls,
            workers,
            delivering: std::sync::Mutex::default(),
        }) {
            let bots = self.clone();
            runtime.spawn(async move { follow(bots).await });
        }
    }

    /// Read the pull request `reference` names in the checkout of `root`, for binding it to a
    /// ticket: a number reads that pull request of the repository, a URL must name it.
    ///
    /// # Errors
    /// Returns the [`PullRefusal`] to tell the caller.
    pub async fn read_pull(
        &self,
        root: &str,
        reference: &str,
    ) -> Result<PullSnapshot, PullRefusal> {
        let reference = PullReference::parse(reference).ok_or(PullRefusal::Reference)?;
        let ports = self.follow_ports().ok_or(PullRefusal::Unsupported)?;
        let snapshot =
            ports
                .pulls
                .read(root, reference.number)
                .await
                .map_err(|error| match error {
                    PullReadError::NotFound => PullRefusal::NotFound,
                    PullReadError::Unavailable(_) => PullRefusal::Unavailable,
                    PullReadError::Failed(_) => PullRefusal::Failed,
                })?;
        if !reference.names(&snapshot) {
            return Err(PullRefusal::OtherRepository);
        }
        Ok(snapshot)
    }

    /// Deliver the inputs waiting on `root`'s ticket `ticket` to its live Agents; inputs for
    /// archived Agents go to the Bot instead. Returns whether nothing is left waiting.
    pub(crate) async fn flush(&self, root: &str, ticket: TicketId) -> bool {
        let Some(ports) = self.follow_ports() else {
            return false;
        };
        let Ok(live) = ports.workers.live(ticket).await else {
            return false;
        };
        let lock = ports.delivery_lock(ticket);
        let _delivering = lock.lock().await;
        let Ok(deliveries) = self.settle(root, ticket, live).await else {
            return false;
        };
        self.send_deliveries(ports, root, ticket, &deliveries).await
    }

    /// Deliver `deliveries` of `root`'s ticket `ticket` as [`Self::send_deliveries`] does,
    /// after any other delivery of that ticket in progress; `true` when every one was
    /// delivered.
    async fn deliver(&self, root: &str, ticket: TicketId, deliveries: &[Delivery]) -> bool {
        let Some(ports) = self.follow_ports() else {
            return deliveries.is_empty();
        };
        let lock = ports.delivery_lock(ticket);
        let _delivering = lock.lock().await;
        self.send_deliveries(ports, root, ticket, deliveries).await
    }

    /// Send `deliveries` of `root`'s ticket `ticket` through `ports`, forget the delivered ones
    /// with the time they were sent, count a failure against each the Agent could not take,
    /// and start a new run of failures for each its busy Agent will take later; `true` when
    /// every one was delivered. The caller holds the ticket's [`Ports::delivery_lock`].
    async fn send_deliveries(
        &self,
        ports: &Ports,
        root: &str,
        ticket: TicketId,
        deliveries: &[Delivery],
    ) -> bool {
        let mut delivered = Vec::with_capacity(deliveries.len());
        let mut failed = Vec::new();
        let mut busy = Vec::new();
        let sent = Utc::now();
        for delivery in deliveries {
            match ports
                .workers
                .deliver(&delivery.agent_id, &delivery.id, &delivery.text)
                .await
            {
                Ok(Delivered::Started) => delivered.push(delivery.id.clone()),
                Ok(Delivered::Busy) => busy.push(delivery.id.clone()),
                Err(error) => {
                    tracing::debug!(?error, %ticket, agent = %delivery.agent_id, "ticket input stays pending");
                    failed.push(delivery.id.clone());
                }
            }
        }
        let all = delivered.len() == deliveries.len();
        if !delivered.is_empty()
            && let Err(error) = self.delivered(root, ticket, delivered, sent).await
        {
            tracing::warn!(?error, %ticket, "delivered ticket inputs stay pending and are sent again");
        }
        if !(failed.is_empty() && busy.is_empty())
            && let Err(error) = self.undelivered(root, ticket, failed, busy).await
        {
            tracing::debug!(?error, %ticket, "failed ticket input deliveries not counted");
        }
        all
    }
}

/// Forge reads made in the last [`READ_WINDOW`], shared by every followed pull request.
#[derive(Debug, Default)]
struct Budget {
    reads: VecDeque<Instant>,
}

impl Budget {
    /// Spend one read at `now`.
    ///
    /// # Errors
    /// Returns when the next read may be made when [`READ_BUDGET`] reads were made in the last
    /// [`READ_WINDOW`].
    fn spend(&mut self, now: Instant) -> Result<(), Instant> {
        while self
            .reads
            .front()
            .is_some_and(|read| now.duration_since(*read) >= READ_WINDOW)
        {
            self.reads.pop_front();
        }
        match self.reads.front() {
            Some(oldest) if self.reads.len() >= READ_BUDGET => Err(*oldest + READ_WINDOW),
            _ => {
                self.reads.push_back(now);
                Ok(())
            }
        }
    }
}

/// When the follower next serves each followed pull request (and each ticket's waiting
/// inputs, under number 0), and the shared read budget.
#[derive(Debug, Default)]
struct Schedule {
    due: BTreeMap<(TicketId, u64), Instant>,
    budget: Budget,
}

/// The follower: wait while no ticket follows anything, else read each due pull request and
/// deliver what waits, one forge read at a time and within the read budget.
async fn follow(bots: Bots) {
    let mut targets = bots.follow_targets();
    let mut schedule = Schedule::default();
    loop {
        let current = targets.borrow_and_update().clone();
        let keys: BTreeSet<(TicketId, u64)> = current
            .iter()
            .flat_map(|target| {
                let pending = target.pending.then_some((target.ticket, 0));
                target
                    .pulls
                    .iter()
                    .map(|pull| (target.ticket, pull.number))
                    .chain(pending)
            })
            .collect();
        schedule.due.retain(|key, _| keys.contains(key));
        for target in &current {
            if bots.is_stopping() {
                return;
            }
            serve(&bots, target, &mut schedule).await;
        }
        let wake = schedule.due.values().min().copied();
        tokio::select! {
            () = bots.stopped() => return,
            changed = targets.changed() => if changed.is_err() { return },
            () = sleep_until(wake) => {}
        }
    }
}

async fn sleep_until(wake: Option<Instant>) {
    match wake {
        Some(wake) => tokio::time::sleep_until(wake).await,
        None => std::future::pending().await,
    }
}

/// Serve one target: retry its waiting inputs when due, then read each due pull request.
async fn serve(bots: &Bots, target: &FollowTarget, schedule: &mut Schedule) {
    let now = Instant::now();
    let pending = (target.ticket, 0);
    if target.pending && schedule.due.get(&pending).is_none_or(|at| *at <= now) {
        bots.flush(&target.root, target.ticket).await;
        schedule
            .due
            .insert(pending, Instant::now() + PENDING_INTERVAL);
    }
    for pull in &target.pulls {
        let key = (target.ticket, pull.number);
        if schedule.due.get(&key).is_some_and(|at| *at > now) {
            continue;
        }
        if let Err(next) = schedule.budget.spend(Instant::now()) {
            schedule.due.insert(key, next);
            continue;
        }
        let next = if read_one(bots, target, pull, &mut schedule.budget).await {
            if target.ended {
                ENDED_INTERVAL
            } else {
                POLL_INTERVAL
            }
        } else {
            backoff(pull.failures.saturating_add(1))
        };
        schedule.due.insert(key, Instant::now() + next);
    }
}

/// Read `pull` of `target`, look up within `budget` the write access of authors the read needs
/// it for, and hand the read to the Bot worker, then deliver what it queued. Returns whether
/// the read succeeded; a failure is recorded on the binding.
async fn read_one(
    bots: &Bots,
    target: &FollowTarget,
    pull: &FollowedPull,
    budget: &mut Budget,
) -> bool {
    let Some(ports) = bots.follow_ports() else {
        return false;
    };
    let snapshot = match ports.pulls.read(&target.root, pull.number).await {
        Ok(snapshot) => snapshot,
        Err(error) => {
            let missing = error == PullReadError::NotFound;
            let message = error.to_string();
            if let Err(error) = bots
                .pull_failed(&target.root, target.ticket, pull.number, message, missing)
                .await
            {
                tracing::debug!(?error, ticket = %target.ticket, "pull request failure not recorded");
            }
            return false;
        }
    };
    let Ok(live) = ports.workers.live(target.ticket).await else {
        return false;
    };
    let authored = authored(ports, &snapshot, &live, pull.mark).await;
    let logins = match bots
        .authors_to_confirm(&target.root, target.ticket, &snapshot)
        .await
    {
        Ok(logins) => logins,
        Err(error) => {
            tracing::debug!(?error, ticket = %target.ticket, "pull request read not applied");
            return false;
        }
    };
    let lookups = lookups(ports, &target.root, logins, budget).await;
    let observed = PullRead {
        root: target.root.clone(),
        ticket: target.ticket,
        snapshot,
        authored,
        lookups,
        live,
    };
    match bots.observe_pull(observed).await {
        Ok(deliveries) => {
            bots.deliver(&target.root, target.ticket, &deliveries).await;
            true
        }
        Err(error) => {
            tracing::debug!(?error, ticket = %target.ticket, "pull request read not applied");
            false
        }
    }
}

/// The entries of `snapshot` the forge account wrote after `mark`, the newest entry the
/// binding handled, that one of the `live` workers sent itself through a tool call; entries
/// up to the mark were handled already. The workers' tool-call inputs are read only when
/// there is such an entry.
async fn authored(
    ports: &Ports,
    snapshot: &PullSnapshot,
    live: &[String],
    mark: Option<DateTime<Utc>>,
) -> BTreeSet<String> {
    let candidates: Vec<_> = snapshot
        .items
        .iter()
        .filter(|item| item.own && mark.is_none_or(|mark| item.created_at > mark))
        .collect();
    if candidates.is_empty() || live.is_empty() {
        return BTreeSet::new();
    }
    let inputs = ports.workers.inputs(live).await;
    candidates
        .into_iter()
        .filter(|item| wrote(&item.body, inputs.iter().map(String::as_str)))
        .map(|item| item.id.clone())
        .collect()
}

/// Whether each of `logins`, the authors of a read whose write access its binding does not
/// know yet, can write to the repository of `root`, one permission lookup per author, each
/// spending one read of `budget`. An author whose lookup failed, or for whom the budget had no room, is reported
/// as failed: the binding holds their entries back and counts the attempt, so a saturated
/// budget or a lookup that cannot succeed holds them for a bounded number of reads only
/// (ADR-287).
async fn lookups(
    ports: &Ports,
    root: &str,
    logins: BTreeSet<String>,
    budget: &mut Budget,
) -> Lookups {
    let mut lookups = Lookups::default();
    for login in logins {
        if budget.spend(Instant::now()).is_err() {
            lookups.failed.insert(login);
            continue;
        }
        match ports.pulls.can_write(root, &login).await {
            Ok(writer) => {
                lookups.answered.insert(login, writer);
            }
            Err(error) => {
                tracing::debug!(?error, %login, "write access lookup failed; entries held back");
                lookups.failed.insert(login);
            }
        }
    }
    lookups
}

/// The wait after `failures` failed reads in a row: the poll interval doubled per failure, at
/// most [`BACKOFF_LIMIT`].
pub(crate) fn backoff(failures: u32) -> Duration {
    POLL_INTERVAL
        .saturating_mul(2_u32.saturating_pow(failures.min(16)))
        .min(BACKOFF_LIMIT)
}

/// Receive the targets the Bot worker publishes.
pub(crate) type Targets = watch::Receiver<Vec<FollowTarget>>;

#[cfg(test)]
mod tests;
