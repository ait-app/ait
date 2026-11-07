use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::{mpsc, watch};
use uuid::Uuid;

use crate::types::BulkRequest;
use crate::{BulkTickets, Center, Error, HubTicket, ManagedRelay, Tokens};

const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub(super) struct Grant {
    pub(super) generation: Uuid,
    pub(super) tokens: Tokens,
    pub(super) session: Uuid,
    pub(super) acceptance: Arc<Acceptance>,
    /// Serve ticket issued with the registration under this JWT generation, used once.
    pub(super) issued: Arc<Mutex<Option<HubTicket>>>,
    /// Bulk lane ticket requests, answered by [`serve_bulk`].
    pub(super) bulk: mpsc::Sender<BulkRequest>,
}

/// A terminal Hub ticket failure under one JWT generation.
#[derive(Debug)]
pub(super) struct Failure {
    generation: Uuid,
    pub(super) error: Error,
}

/// Successful hub ticket issuance proves that the endpoint accepted the current JWT generation.
#[derive(Debug, Default)]
pub(super) struct Acceptance(AtomicBool);

impl Acceptance {
    /// Return whether the hub ticket endpoint accepted this generation, retaining evidence
    /// across socket disconnects.
    pub(super) fn accepted(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Keep the Hub link of the current grant up until the grant changes.
pub(super) async fn maintain<C: Center, R: ManagedRelay>(
    center: &C,
    relay: &R,
    mut grants: watch::Receiver<Option<Grant>>,
    errors: mpsc::Sender<Failure>,
) {
    loop {
        let grant = grants.borrow_and_update().clone();
        let Some(grant) = grant else {
            if grants.changed().await.is_err() {
                return;
            }
            continue;
        };
        tokio::select! {
            // Revocation and token/session replacement cancel outstanding ticket/start work.
            biased;
            changed = grants.changed() => {
                if changed.is_err() {
                    return;
                }
            }
            error = maintain_grant(center, relay, &grant) => {
                if errors.send(Failure { generation: grant.generation, error }).await.is_err()
                    || grants.changed().await.is_err()
                {
                    return;
                }
            }
        }
    }
}

async fn maintain_grant<C: Center, R: ManagedRelay>(center: &C, relay: &R, grant: &Grant) -> Error {
    const INITIAL: u64 = 5;
    const MAXIMUM: u64 = 60;
    let mut delay = INITIAL;
    let mut retry_at = tokio::time::Instant::now();
    loop {
        if relay.hub_online().await {
            // Starting a connector is not success: only a completed handshake resets backoff.
            delay = INITIAL;
            retry_at = tokio::time::Instant::now();
        } else if !relay.hub_active().await && tokio::time::Instant::now() >= retry_at {
            let result = tokio::time::timeout(ATTEMPT_TIMEOUT, connect(center, relay, grant))
                .await
                .unwrap_or(Err(Error::Unavailable));
            if let Err(
                error @ (Error::AccessRejected
                | Error::Unauthorized
                | Error::SessionExpired
                | Error::Conflict),
            ) = result
            {
                return error;
            }
            // Keep backoff after async starts too: their socket handshake can fail later.
            retry_at = tokio::time::Instant::now() + Duration::from_secs(delay);
            delay = (delay * 2).min(MAXIMUM);
        }
        // Observe successful handshakes promptly without issuing new tickets while connected.
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn connect<C: Center, R: ManagedRelay>(
    center: &C,
    relay: &R,
    grant: &Grant,
) -> Result<(), Error> {
    if !relay.hub_active().await {
        let issued = grant
            .issued
            .lock()
            .ok()
            .and_then(|mut issued| issued.take())
            .filter(HubTicket::usable);
        let ticket = match issued {
            Some(ticket) => ticket,
            None => center.hub_ticket(&grant.tokens, grant.session).await?,
        };
        grant.acceptance.0.store(true, Ordering::Relaxed);
        let bulk = BulkTickets::new(grant.bulk.clone(), grant.session);
        relay.start_hub(grant.session, ticket, Some(bulk)).await?;
    }
    Ok(())
}

/// Answer the hub link's bulk lane ticket requests with the latest grant's JWT,
/// so a link that outlived token rotation still presents a current one. Requests
/// for a session that is no longer published are refused.
pub(super) async fn serve_bulk<C: Center>(
    center: &C,
    grants: watch::Receiver<Option<Grant>>,
    mut requests: mpsc::Receiver<BulkRequest>,
) {
    while let Some(request) = requests.recv().await {
        let tokens = grants
            .borrow()
            .as_ref()
            .filter(|grant| grant.session == request.session)
            .map(|grant| grant.tokens.clone());
        let result = match tokens {
            Some(tokens) => tokio::time::timeout(
                ATTEMPT_TIMEOUT,
                center.hub_bulk_ticket(&tokens, request.session),
            )
            .await
            .unwrap_or(Err(Error::Unavailable)),
            None => Err(Error::Unavailable),
        };
        let _ = request.reply.send(result);
    }
}

pub(super) async fn next_error(
    errors: &mut mpsc::Receiver<Failure>,
    generation: Option<Uuid>,
) -> Failure {
    while let Some(failure) = errors.recv().await {
        if Some(failure.generation) == generation {
            return failure;
        }
    }
    std::future::pending().await
}
