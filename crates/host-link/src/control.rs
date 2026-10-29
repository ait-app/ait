//! Authenticated adapters submit credentials here; only the runtime owns their durable rotation.

use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crate::{Binding, Credential, Error, Pending, State};

/// A write-only authorization handoff. No API response returns these secrets.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Authorization {
    /// One-use enrollment issued by the center's Host page.
    Enrollment {
        /// Secret enrollment JWT.
        token: SecretString,
        /// Stable identity retained when the caller retries this handoff.
        request_id: Uuid,
    },
    /// An approved device flow; the daemon claims and persists the machine credentials.
    DeviceCode {
        /// Secret center polling code.
        device_code: SecretString,
        /// Public approval code used only for recovery/status on the CLI.
        user_code: String,
        /// Stable identity for crash-safe issuance.
        request_id: Uuid,
        /// Original authorization deadline.
        expires_at: DateTime<Utc>,
        /// Minimum polling interval in seconds.
        interval: u64,
    },
    /// An existing machine credential, bound to this daemon's stable identity.
    Credential {
        /// Stable external handoff identity; retry with the same ID after a lost response.
        request_id: Uuid,
        /// Rotating refresh secret, handed over exclusively to this daemon.
        refresh_token: SecretString,
        /// Absolute refresh expiry.
        refresh_expires_at: DateTime<Utc>,
        /// Center assignment, verified again by the next refresh response.
        binding: Binding,
    },
}

impl Authorization {
    fn validate(&self, server: Uuid) -> Result<(), Error> {
        let valid = match self {
            Self::Enrollment { token, request_id } => valid_secret(token) && !request_id.is_nil(),
            Self::DeviceCode {
                device_code,
                user_code,
                request_id,
                expires_at,
                interval,
            } => {
                device_code.expose_secret().len() == 64
                    && device_code
                        .expose_secret()
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit())
                    && user_code.len() == 14
                    && user_code
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() || b == b'-')
                    && !request_id.is_nil()
                    && *expires_at > Utc::now()
                    && *expires_at <= Utc::now() + chrono::Duration::minutes(10)
                    && (5..=300).contains(interval)
            }
            Self::Credential {
                request_id,
                refresh_token,
                refresh_expires_at,
                binding,
            } => {
                !request_id.is_nil()
                    && valid_secret(refresh_token)
                    && *refresh_expires_at > Utc::now()
                    && binding.server_id == server
                    && [binding.node_id, binding.host_id, binding.grant_id]
                        .iter()
                        .all(|id| !id.is_nil())
            }
        };
        if valid { Ok(()) } else { Err(Error::Protocol) }
    }

    pub(crate) fn apply(self, state: &mut State) {
        state.disconnected = false;
        state.authorization_id = Some(self.id());
        state.credential = None;
        state.pending = match self {
            Self::Enrollment { token, request_id } => {
                Some(Pending::Enrollment { token, request_id })
            }
            Self::DeviceCode {
                device_code,
                user_code,
                request_id,
                expires_at,
                interval,
            } => Some(Pending::Web {
                device_code,
                user_code,
                request_id,
                expires_at,
                interval,
            }),
            Self::Credential {
                refresh_token,
                refresh_expires_at,
                binding,
                ..
            } => {
                state.credential = Some(Credential {
                    refresh_token,
                    refresh_expires_at,
                    binding,
                });
                None
            }
        };
    }

    pub(crate) fn id(&self) -> Uuid {
        match self {
            Self::Enrollment { request_id, .. }
            | Self::DeviceCode { request_id, .. }
            | Self::Credential { request_id, .. } => *request_id,
        }
    }
}

fn valid_secret(secret: &SecretString) -> bool {
    let value = secret.expose_secret();
    !value.is_empty() && value.len() <= 8192 && !value.chars().any(char::is_control)
}

/// Whether authorization was explicitly requested or is a one-time automatic bootstrap.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthorizationMode {
    /// User/API login may replace the existing machine authorization.
    #[default]
    Explicit,
    /// Only initialize a never-authorized, never-disconnected daemon; otherwise leave it unchanged.
    Initial,
}

pub(crate) enum Command {
    Authorize(
        Authorization,
        AuthorizationMode,
        oneshot::Sender<Result<(), Error>>,
    ),
    Disconnect(oneshot::Sender<Result<(), Error>>),
}

/// Cloneable control capability for the daemon's authenticated HTTP/RPC adapters.
#[derive(Debug, Clone)]
pub struct Handle {
    server: Uuid,
    sender: mpsc::Sender<Command>,
}

impl Handle {
    pub(crate) fn channel(server: Uuid) -> (Self, mpsc::Receiver<Command>) {
        let (sender, receiver) = mpsc::channel(8);
        (Self { server, sender }, receiver)
    }

    /// Persist authorization before allowing the autonomous controller to consume it.
    /// # Errors
    /// Rejects invalid/mismatched credentials, unavailable runtime, or failed durable writes.
    pub async fn authorize(
        &self,
        authorization: Authorization,
        mode: AuthorizationMode,
    ) -> Result<(), Error> {
        authorization.validate(self.server)?;
        let (reply, result) = oneshot::channel();
        self.send(Command::Authorize(authorization, mode, reply), result)
            .await
    }

    /// Disconnect Hub and Relay, stop renewal, and durably discard every machine credential.
    /// # Errors
    /// Returns `Storage` if removal could not be persisted, or `Unavailable` on shutdown.
    pub async fn disconnect(&self) -> Result<(), Error> {
        let (reply, result) = oneshot::channel();
        self.send(Command::Disconnect(reply), result).await
    }

    async fn send(
        &self,
        command: Command,
        result: oneshot::Receiver<Result<(), Error>>,
    ) -> Result<(), Error> {
        self.sender
            .send(command)
            .await
            .map_err(|_| Error::Unavailable)?;
        result.await.map_err(|_| Error::Unavailable)?
    }
}
