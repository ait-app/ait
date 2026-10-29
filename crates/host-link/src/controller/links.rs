use std::time::Duration;

use tokio::sync::{mpsc, watch};
use uuid::Uuid;

use crate::{Center, Error, ManagedRelay, Tokens};

const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub(super) struct Grant {
    pub(super) generation: Uuid,
    pub(super) tokens: Tokens,
    pub(super) session: Uuid,
}

#[derive(Debug)]
pub(super) struct Failure {
    generation: Uuid,
    error: Error,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum Kind {
    Relay,
    Hub,
}

pub(super) async fn maintain<C: Center, R: ManagedRelay>(
    center: &C,
    relay: &R,
    mut grants: watch::Receiver<Option<Grant>>,
    errors: mpsc::Sender<Failure>,
    kind: Kind,
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
            error = maintain_grant(center, relay, &grant, kind) => {
                if errors.send(Failure { generation: grant.generation, error }).await.is_err()
                    || grants.changed().await.is_err()
                {
                    return;
                }
            }
        }
    }
}

async fn maintain_grant<C: Center, R: ManagedRelay>(
    center: &C,
    relay: &R,
    grant: &Grant,
    kind: Kind,
) -> Error {
    let (initial, maximum) = match kind {
        Kind::Relay => (1, 30),
        Kind::Hub => (5, 60),
    };
    let mut delay = initial;
    let mut retry_at = tokio::time::Instant::now();
    loop {
        let (online, active) = match kind {
            Kind::Relay => (relay.online().await, relay.active().await),
            Kind::Hub => (relay.hub_online().await, relay.hub_active().await),
        };
        if online {
            // Starting a connector is not success: only a completed handshake resets backoff.
            delay = initial;
            retry_at = tokio::time::Instant::now();
        } else if !active && tokio::time::Instant::now() >= retry_at {
            let result = tokio::time::timeout(ATTEMPT_TIMEOUT, connect(center, relay, grant, kind))
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
            delay = (delay * 2).min(maximum);
        }
        // Observe successful handshakes promptly without issuing new tickets while connected.
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn connect<C: Center, R: ManagedRelay>(
    center: &C,
    relay: &R,
    grant: &Grant,
    kind: Kind,
) -> Result<(), Error> {
    match kind {
        Kind::Relay => {
            if !relay.active().await {
                let ticket = center.ticket(&grant.tokens, grant.session).await?;
                relay.start(grant.session, ticket).await?;
            }
        }
        Kind::Hub => {
            if !relay.hub_active().await {
                let ticket = center.hub_ticket(&grant.tokens, grant.session).await?;
                relay.start_hub(grant.session, ticket).await?;
            }
        }
    }
    Ok(())
}

pub(super) async fn next_error(
    errors: &mut mpsc::Receiver<Failure>,
    generation: Option<Uuid>,
) -> Error {
    while let Some(failure) = errors.recv().await {
        if Some(failure.generation) == generation {
            return failure.error;
        }
    }
    std::future::pending().await
}
