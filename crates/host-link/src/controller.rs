use std::time::Duration;

use chrono::Utc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{Center, CredentialStore, Error, ManagedRelay, Pending, Session, State, Tokens};

/// Serializes refresh rotation and maintains runtime leases independently of a GUI.
pub struct Controller<C, S, R> {
    center: C,
    store: S,
    relay: R,
    state: State,
    instance: Uuid,
    active_session: Option<(Tokens, Uuid)>,
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
        self.relay.report(
            "connecting",
            self.state.credential.as_ref().map(|c| &c.binding),
        );
        let result = tokio::select! {
            () = cancel.cancelled() => Ok(()),
            result = self.maintain(&cancel) => result,
        };
        self.relay.stop().await;
        if let Some((tokens, session)) = &self.active_session {
            let _ = self.center.close(tokens, *session).await;
        }
        self.relay.report(
            match result {
                Ok(()) => "stopped",
                Err(Error::Storage) => "storage_error",
                Err(Error::Unauthorized) => "reauthorization_required",
                Err(Error::Conflict) => "runtime_conflict",
                Err(_) => "protocol_error",
            },
            self.state.credential.as_ref().map(|c| &c.binding),
        );
        result
    }

    async fn refresh(&mut self) -> Result<Tokens, Error> {
        let credential = self.state.credential.as_ref().ok_or(Error::Unauthorized)?;
        let request_id = match self.state.pending {
            // A saved request may have committed before expiry. Only the center can decide
            // whether its bounded receipt is still recoverable after the source expired.
            Some(Pending::Refresh(id)) => id,
            None if credential.refresh_expires_at <= Utc::now() => {
                return Err(Error::Unauthorized);
            }
            None => Uuid::new_v4(),
            Some(_) => return Err(Error::Unauthorized),
        };
        self.state.pending = Some(Pending::Refresh(request_id));
        self.store.save(&self.state)?;
        let tokens = self
            .center
            .refresh(&credential.refresh_token, request_id)
            .await?;
        if tokens.credential.binding != credential.binding {
            return Err(Error::Protocol);
        }
        let mut next = self.state.clone();
        next.credential = Some(tokens.credential.clone());
        next.pending = None;
        self.store.save(&next)?;
        self.state = next;
        Ok(tokens)
    }

    async fn initial_tokens(&mut self, cancel: &CancellationToken) -> Result<Tokens, Error> {
        let mut delay = 1;
        loop {
            match self.refresh().await {
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
        let (access_until, refresh_at) = token_schedule(&tokens);
        let mut publication = Publication {
            tokens,
            registration: Uuid::new_v4(),
            session: None,
            next_renew: tokio::time::Instant::now(),
            access_until,
            refresh_at,
            lease_until: None,
        };
        let mut retry_at = tokio::time::Instant::now();
        let mut delay = 1;
        loop {
            if cancel.is_cancelled() {
                return Ok(());
            }
            let now = tokio::time::Instant::now();
            let expires = publication
                .lease_until
                .map_or(publication.access_until, |lease| {
                    lease.min(publication.access_until)
                });
            if expires <= now {
                self.relay.stop().await;
            }
            if tokio::time::Instant::now() < retry_at {
                pause(cancel, 1).await;
                continue;
            }
            // Do not leave remote transport open across its local authorization deadline while
            // an HTTP operation is in flight. Dropped refresh requests retain their persisted ID.
            let window = expires.checked_duration_since(now);
            let attempt = if let Some(window) = window {
                tokio::time::timeout(window, self.step(&mut publication))
                    .await
                    .unwrap_or(Err(Error::Unavailable))
            } else {
                self.step(&mut publication).await
            };
            match attempt {
                Ok(()) => {
                    delay = 1;
                    pause(cancel, 1).await;
                }
                Err(Error::SessionExpired) => {
                    self.relay.stop().await;
                    publication.session = None;
                    publication.lease_until = None;
                    self.active_session = None;
                    publication.registration = Uuid::new_v4();
                }
                Err(Error::Conflict) => {
                    // A crashed predecessor can retain its lease after a service restart.
                    // Wait for the center to release it; never steal or replace its authority.
                    self.relay.stop().await;
                    self.relay.report(
                        "runtime_conflict",
                        Some(&publication.tokens.credential.binding),
                    );
                    retry_at = tokio::time::Instant::now() + Duration::from_secs(30);
                }
                Err(Error::Unavailable) => {
                    if expires <= tokio::time::Instant::now() {
                        self.relay.stop().await;
                    }
                    self.relay
                        .report("retrying", Some(&publication.tokens.credential.binding));
                    retry_at = tokio::time::Instant::now() + Duration::from_secs(delay);
                    delay = (delay * 2).min(30);
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn step(&mut self, publication: &mut Publication) -> Result<(), Error> {
        if tokio::time::Instant::now() >= publication.refresh_at {
            publication.tokens = self.refresh().await?;
            (publication.access_until, publication.refresh_at) =
                token_schedule(&publication.tokens);
            // A recovered receipt retains its original expiry. Persist its successor before
            // rotating again, and do not register with an expired JWT.
            if publication.access_until <= tokio::time::Instant::now() {
                return Ok(());
            }
            publication.next_renew = tokio::time::Instant::now();
        }
        if publication.session.is_none() {
            let session = self
                .center
                .register(
                    &publication.tokens,
                    &self.state.machine,
                    self.instance,
                    publication.registration,
                )
                .await?;
            // Registration recovery reports a duration, not the existing lease's absolute end.
            // Renew immediately to learn its authoritative deadline before opening transport.
            publication.next_renew = tokio::time::Instant::now();
            publication.session = Some(session);
        }
        let active = publication.session.as_mut().ok_or(Error::Protocol)?;
        self.active_session = Some((publication.tokens.clone(), active.id));
        // An ambiguous renewal may have committed at the center. Retry the known session
        // before creating a competing registration; only a terminal session error discards it.
        if tokio::time::Instant::now() >= publication.next_renew
            || publication
                .lease_until
                .is_some_and(|until| until <= tokio::time::Instant::now())
        {
            active.lease_until = self.center.renew(&publication.tokens, active.id).await?;
            publication.lease_until = Some(
                tokio::time::Instant::now()
                    + (active.lease_until - Utc::now())
                        .to_std()
                        .unwrap_or_default(),
            );
            publication.next_renew =
                tokio::time::Instant::now() + Duration::from_secs(active.renew_after_seconds);
        }
        if !self.relay.active().await {
            let ticket = self.center.ticket(&publication.tokens, active.id).await?;
            self.relay.start(active.id, ticket).await?;
        }
        self.relay
            .report("running", Some(&publication.tokens.credential.binding));
        Ok(())
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
