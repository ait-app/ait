use std::time::Duration;

use tokio::sync::{mpsc, watch};
use uuid::Uuid;

use crate::{Center, Error, ManagedRelay, Tokens};

const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(10);
const HUB_RETRY: Duration = Duration::from_secs(60);

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
    let mut delay = 1;
    loop {
        let result = tokio::time::timeout(ATTEMPT_TIMEOUT, connect(center, relay, grant, kind))
            .await
            .unwrap_or(Err(Error::Unavailable));
        let wait = match result {
            Ok(()) => {
                delay = 1;
                Duration::from_secs(1)
            }
            Err(error) => match (kind, error) {
                // Protocol/availability failures belong to one transport; an explicit loss
                // of authority must instead be handled for the whole publication.
                (_, Error::Unauthorized | Error::SessionExpired | Error::Conflict) => return error,
                (Kind::Hub, _) => HUB_RETRY,
                (Kind::Relay, _) => {
                    let wait = Duration::from_secs(delay);
                    delay = (delay * 2).min(30);
                    wait
                }
            },
        };
        tokio::time::sleep(wait).await;
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
