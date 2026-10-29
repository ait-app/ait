use std::time::Duration;

use chrono::Utc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{Center, CredentialStore, Error, ManagedRelay, Session, State, Tokens};

mod authority;
mod links;

/// Serializes refresh rotation and maintains runtime leases independently of a GUI.
pub struct Controller<C, S, R> {
    pub(crate) center: C,
    pub(crate) store: S,
    pub(crate) relay: R,
    pub(crate) state: State,
    instance: Uuid,
    pub(crate) active_session: Option<(Tokens, Uuid)>,
}

#[derive(Debug)]
struct Publication {
    tokens: Tokens,
    registration: Uuid,
    session: Option<Session>,
    next_renew: tokio::time::Instant,
    access_until: tokio::time::Instant,
    refresh_at: tokio::time::Instant,
    lease_until: Option<tokio::time::Instant>,
    generation: Uuid,
}

impl<C: Center, S: CredentialStore, R: ManagedRelay> Controller<C, S, R> {
    /// Assemble a manager with an exclusively locked private store and claimed Relay handle.
    #[must_use]
    pub fn new(center: C, store: S, relay: R, state: State, instance: Uuid) -> Self {
        Self {
            center,
            store,
            relay,
            state,
            instance,
            active_session: None,
        }
    }

    /// Stay connected until cancellation or a terminal authorization/storage failure.
    ///
    /// Refresh responses are committed before use. Outages are retried with the same pending ID;
    /// expired authorization or leases immediately close remote transport.
    /// # Errors
    /// Requires login for revoked/expired credentials; storage failures pause rotation.
    pub async fn run(mut self, cancel: CancellationToken) -> Result<(), Error> {
        self.run_until(cancel).await
    }

    pub(crate) async fn run_until(&mut self, cancel: CancellationToken) -> Result<(), Error> {
        self.relay.report(
            "connecting",
            self.state.credential.as_ref().map(|c| &c.binding),
        );
        let result = tokio::select! {
            biased;
            () = cancel.cancelled() => Ok(()),
            result = self.maintain(&cancel) => result,
        };
        self.relay.stop().await;
        if let Some((tokens, session)) = self.active_session.take() {
            let _ =
                tokio::time::timeout(Duration::from_secs(5), self.center.close(&tokens, session))
                    .await;
        }
        self.relay.report(
            match result {
                Ok(()) => "stopped",
                Err(Error::Storage) => "storage_error",
                Err(Error::Unauthorized | Error::Denied) => "reauthorization_required",
                Err(Error::Conflict) => "runtime_conflict",
                Err(_) => "protocol_error",
            },
            self.state.credential.as_ref().map(|c| &c.binding),
        );
        result
    }

    async fn refresh(&mut self) -> Result<Tokens, Error> {
        crate::authorization::restore(&self.center, &self.store, &mut self.state).await
    }

    async fn initial_tokens(&mut self, cancel: &CancellationToken) -> Result<Tokens, Error> {
        let mut delay = 1;
        loop {
            let prepared = if matches!(
                self.state.pending,
                Some(crate::Pending::Enrollment { .. } | crate::Pending::Web { .. })
            ) {
                crate::authorization::complete(&self.center, &self.store, &mut self.state, cancel)
                    .await
            } else {
                Ok(())
            };
            let result = match prepared {
                Ok(()) => self.refresh().await,
                Err(error) => Err(error),
            };
            match result {
                Ok(tokens) => return Ok(tokens),
                Err(Error::Unavailable) => {
                    self.relay.report(
                        "retrying",
                        self.state.credential.as_ref().map(|c| &c.binding),
                    );
                    pause(cancel, delay).await;
                    delay = (delay * 2).min(30);
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn maintain(&mut self, cancel: &CancellationToken) -> Result<(), Error> {
        let tokens = self.initial_tokens(cancel).await?;
        let (grants, receiver) = tokio::sync::watch::channel(None);
        let (errors, failures) = tokio::sync::mpsc::channel(2);
        let mut authority = authority::Authority {
            center: &self.center,
            store: &self.store,
            relay: &self.relay,
            state: &mut self.state,
            instance: self.instance,
            active_session: &mut self.active_session,
            grants,
            errors: failures,
        };
        let relay = links::maintain(
            &self.center,
            &self.relay,
            receiver.clone(),
            errors.clone(),
            links::Kind::Relay,
        );
        let hub = links::maintain(
            &self.center,
            &self.relay,
            receiver,
            errors,
            links::Kind::Hub,
        );
        // All three futures are scoped to this manager. On exit they are dropped before stop(),
        // so an outstanding ticket can never restart transport after shutdown.
        tokio::select! {
            // Revoke expired authority before polling a ticket that becomes ready at the deadline.
            biased;
            result = authority.maintain(tokens, cancel) => result,
            () = relay => Err(Error::Protocol),
            () = hub => Err(Error::Protocol),
        }
    }
}

fn token_schedule(tokens: &Tokens) -> (tokio::time::Instant, tokio::time::Instant) {
    let remaining = (tokens.access_expires_at - Utc::now())
        .to_std()
        .unwrap_or_default();
    let now = tokio::time::Instant::now();
    // Near an account deadline, avoid rotating every second; retain original absolute expiry.
    let refresh_delay = remaining
        .saturating_sub(Duration::from_secs(60))
        .max(remaining / 2);
    (now + remaining, now + refresh_delay)
}

async fn pause(cancel: &CancellationToken, seconds: u64) {
    tokio::select! {
        () = cancel.cancelled() => {},
        () = tokio::time::sleep(Duration::from_secs(seconds)) => {},
    }
}

#[cfg(test)]
mod tests;
