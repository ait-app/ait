use std::time::Duration;

use chrono::Utc;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{Publication, links, token_schedule};
use crate::{Center, CredentialStore, Error, ManagedRelay, State, Tokens};

pub(super) struct Authority<'a, C, S, R> {
    pub(super) center: &'a C,
    pub(super) store: &'a S,
    pub(super) relay: &'a R,
    pub(super) state: &'a mut State,
    pub(super) instance: Uuid,
    pub(super) active_session: &'a mut Option<(Tokens, Uuid)>,
    pub(super) grants: watch::Sender<Option<links::Grant>>,
    pub(super) errors: mpsc::Receiver<links::Failure>,
}

impl<C: Center, S: CredentialStore, R: ManagedRelay> Authority<'_, C, S, R> {
    pub(super) async fn maintain(
        &mut self,
        tokens: Tokens,
        cancel: &CancellationToken,
    ) -> Result<(), Error> {
        let (access_until, refresh_at) = token_schedule(&tokens);
        let mut publication = Publication {
            tokens,
            registration: Uuid::new_v4(),
            session: None,
            next_renew: tokio::time::Instant::now(),
            access_until,
            refresh_at,
            lease_until: None,
            generation: Uuid::new_v4(),
        };
        let mut retry_at = tokio::time::Instant::now();
        let mut delay = 1;
        loop {
            if cancel.is_cancelled() {
                return Ok(());
            }
            let now = tokio::time::Instant::now();
            let expires = publication.expires();
            if expires <= now {
                self.revoke(&mut publication).await;
            }
            // Do not leave remote transport open across its local authorization deadline while
            // an HTTP operation is in flight. Dropped refresh requests retain their persisted ID.
            let window = expires.checked_duration_since(now);
            let generation = self.grants.borrow().as_ref().map(|grant| grant.generation);
            // Split borrows so terminal transport errors can interrupt an authority request.
            let errors = &mut self.errors;
            let mut lease = Lease {
                center: self.center,
                store: self.store,
                state: self.state,
                instance: self.instance,
                active_session: self.active_session,
            };
            let attempt = tokio::select! {
                error = links::next_error(errors, generation) => {
                    // Refresh may have committed while renewal is still awaiting its response.
                    // Errors from the previous JWT must not revoke that successor authority.
                    if generation != Some(publication.generation) {
                        continue;
                    }
                    Err(error)
                },
                result = async {
                    if tokio::time::Instant::now() < retry_at {
                        pause(cancel, expires).await;
                        return None;
                    }
                    Some(if let Some(window) = window {
                        tokio::time::timeout(window, lease.step(&mut publication))
                            .await
                            .unwrap_or(Err(Error::Unavailable))
                    } else {
                        lease.step(&mut publication).await
                    })
                } => match result {
                    Some(result) => result,
                    None => continue,
                },
            };
            match attempt {
                Ok(()) => {
                    self.publish(&publication);
                    self.relay
                        .report("running", Some(&publication.tokens.credential.binding));
                    delay = 1;
                    pause(cancel, publication.expires()).await;
                }
                Err(Error::SessionExpired) => {
                    self.revoke(&mut publication).await;
                    publication.session = None;
                    publication.lease_until = None;
                    *self.active_session = None;
                    publication.registration = Uuid::new_v4();
                }
                Err(Error::Conflict) => {
                    // A crashed predecessor can retain its lease after a service restart.
                    // Wait for the center to release it; never steal or replace its authority.
                    self.revoke(&mut publication).await;
                    self.relay.report(
                        "runtime_conflict",
                        Some(&publication.tokens.credential.binding),
                    );
                    retry_at = tokio::time::Instant::now() + Duration::from_secs(30);
                }
                Err(Error::Unavailable) => {
                    if expires <= tokio::time::Instant::now() {
                        self.revoke(&mut publication).await;
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

    async fn revoke(&self, publication: &mut Publication) {
        publication.generation = Uuid::new_v4();
        self.grants.send_replace(None);
        self.relay.stop().await;
    }

    fn publish(&self, publication: &Publication) {
        let now = tokio::time::Instant::now();
        if publication.access_until <= now
            || publication.lease_until.is_none_or(|until| until <= now)
        {
            return;
        }
        if self
            .grants
            .borrow()
            .as_ref()
            .is_some_and(|grant| grant.generation == publication.generation)
        {
            return;
        }
        if let Some(session) = &publication.session {
            self.grants.send_replace(Some(links::Grant {
                generation: publication.generation,
                tokens: publication.tokens.clone(),
                session: session.id,
            }));
        }
    }
}

impl Publication {
    fn expires(&self) -> tokio::time::Instant {
        self.lease_until
            .map_or(self.access_until, |lease| lease.min(self.access_until))
    }
}

async fn pause(cancel: &CancellationToken, expires: tokio::time::Instant) {
    let now = tokio::time::Instant::now();
    let next_poll = now + Duration::from_secs(1);
    // Once revoked, an expired deadline must not turn outage backoff into a busy loop.
    let wake = if expires > now {
        next_poll.min(expires)
    } else {
        next_poll
    };
    tokio::select! {
        () = cancel.cancelled() => {},
        () = tokio::time::sleep_until(wake) => {},
    }
}

struct Lease<'a, C, S> {
    center: &'a C,
    store: &'a S,
    state: &'a mut State,
    instance: Uuid,
    active_session: &'a mut Option<(Tokens, Uuid)>,
}

impl<C: Center, S: CredentialStore> Lease<'_, C, S> {
    async fn step(&mut self, publication: &mut Publication) -> Result<(), Error> {
        if tokio::time::Instant::now() >= publication.refresh_at {
            publication.tokens =
                crate::authorization::restore(self.center, self.store, self.state).await?;
            publication.generation = Uuid::new_v4();
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
        *self.active_session = Some((publication.tokens.clone(), active.id));
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
        Ok(())
    }
}
