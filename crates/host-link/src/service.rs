//! One daemon-owned publication lifecycle, including authorization and complete disconnection.

use std::time::Duration;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::control::{AuthorizationMode, Command, Handle};
use crate::controller::Controller;
use crate::{Center, CredentialStore, Error, ManagedRelay, State};

/// Long-lived publication owner, available before login and after authorization failures.
pub struct Service<C, S, R> {
    controller: Controller<C, S, R>,
    handle: Handle,
    commands: mpsc::Receiver<Command>,
}

impl<C: Center, S: CredentialStore, R: ManagedRelay> Service<C, S, R> {
    /// Own `state` and its exclusive `store`; publish this runtime's `instance` through `relay`.
    #[must_use]
    pub fn new(center: C, store: S, relay: R, state: State, instance: Uuid) -> Self {
        let (handle, commands) = Handle::channel(state.machine.server_id);
        Self {
            controller: Controller::new(center, store, relay, state, instance),
            handle,
            commands,
        }
    }

    /// Return a control handle for authenticated adapters; it contains no credentials.
    #[must_use]
    pub fn handle(&self) -> Handle {
        self.handle.clone()
    }

    /// Serve authorization updates and maintain publication until `cancel` is cancelled.
    /// Terminal failures leave local control available; shutdown preserves login, disconnect clears it.
    pub async fn run(mut self, cancel: CancellationToken) {
        let mut active = self.authorized();
        if !active {
            self.controller.relay.report(
                if self.controller.state.disconnected {
                    "disconnected"
                } else {
                    "unauthorized"
                },
                None,
            );
        }
        while !cancel.is_cancelled() {
            let command = if active {
                self.maintain(&cancel).await
            } else {
                tokio::select! {
                    biased;
                    () = cancel.cancelled() => None,
                    command = self.commands.recv() => command,
                }
            };
            active = self.execute(command).await;
        }
        self.controller.relay.stop().await;
    }

    fn authorized(&self) -> bool {
        !self.controller.state.disconnected
            && (self.controller.state.credential.is_some()
                || self.controller.state.pending.is_some())
    }

    async fn execute(&mut self, command: Option<Command>) -> bool {
        match command {
            Some(Command::Authorize(authorization, mode, reply)) => {
                if mode == AuthorizationMode::Initial
                    && (self.controller.state.disconnected || self.authorized())
                {
                    let _ = reply.send(Ok(()));
                    return false;
                }
                if self.controller.state.authorization_id == Some(authorization.id()) {
                    let _ = reply.send(Ok(()));
                    return self.authorized();
                }
                let mut next = self.controller.state.clone();
                authorization.apply(&mut next);
                let result = self.controller.store.save(&next);
                if result.is_ok() {
                    self.controller.state = next;
                    self.controller.relay.report(
                        "connecting",
                        self.controller
                            .state
                            .credential
                            .as_ref()
                            .map(|c| &c.binding),
                    );
                }
                // A failed replacement preserves the previous authorization unless explicitly disconnected.
                let _ = reply.send(result);
                self.authorized()
            }
            Some(Command::Disconnect(reply)) => {
                let result = self.disconnect().await;
                let _ = reply.send(result);
                false
            }
            None => false,
        }
    }

    async fn disconnect(&mut self) -> Result<(), Error> {
        // Workers, access JWTs, ticket fetches and the old lease have already been drained.
        self.controller.relay.stop().await;
        let credential = self.controller.state.disconnect();
        let result = self.controller.store.save(&self.controller.state);
        self.controller.relay.report(
            if result.is_ok() {
                "disconnected"
            } else {
                "storage_error"
            },
            None,
        );
        if let Some(credential) = credential {
            // Remote cleanup must not retain a secret or prevent local sign-out during an outage.
            let _ = tokio::time::timeout(
                Duration::from_secs(5),
                self.controller.center.revoke(&credential.refresh_token),
            )
            .await;
        }
        result
    }

    async fn maintain(&mut self, cancel: &CancellationToken) -> Option<Command> {
        let authorization_id = self.controller.state.authorization_id;
        let session_cancel = cancel.child_token();
        let running = self.controller.run_until(session_cancel.clone());
        tokio::pin!(running);
        loop {
            tokio::select! {
                biased;
                _result = &mut running => return None,
                command = self.commands.recv() => match command {
                    Some(Command::Authorize(authorization, mode, reply)) if mode == AuthorizationMode::Initial || authorization_id == Some(authorization.id()) => {
                        let _ = reply.send(Ok(()));
                    }
                    command => {
                        // Drain cleanup before changing authorization; no old ticket can restart a link.
                        session_cancel.cancel();
                        let _ = running.await;
                        return command;
                    }
                },
            }
        }
    }
}
