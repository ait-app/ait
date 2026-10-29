//! Durable credential restoration shared by login and unattended runtime.

use std::time::Duration;

use chrono::Utc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{Center, CredentialStore, Error, Pending, State, Tokens};

/// Restore access by rotating the saved credential, replaying unfinished requests unchanged.
///
/// # Arguments
/// * `center` - Machine authentication service.
/// * `store` - Exclusively owned durable credential storage.
/// * `state` - Saved installation and rotation state, updated only after durable commit.
/// # Returns
/// The recovered token pair; an old receipt can retain an expired access deadline.
/// # Errors
/// Returns authorization, transport, protocol, or storage failures without discarding recovery state.
pub async fn restore(
    center: &impl Center,
    store: &impl CredentialStore,
    state: &mut State,
) -> Result<Tokens, Error> {
    let credential = state.credential.as_ref().ok_or(Error::Unauthorized)?;
    let request_id = match state.pending {
        // A saved request may have committed before expiry. Only the center can decide
        // whether its bounded receipt is still recoverable after the source expired.
        Some(Pending::Refresh(id)) => id,
        None if credential.refresh_expires_at <= Utc::now() => {
            return Err(Error::Unauthorized);
        }
        None => Uuid::new_v4(),
        Some(_) => return Err(Error::Unauthorized),
    };
    state.pending = Some(Pending::Refresh(request_id));
    store.save(state)?;
    let tokens = center
        .refresh(&credential.refresh_token, request_id)
        .await?;
    if tokens.credential.binding != credential.binding {
        return Err(Error::Protocol);
    }
    let mut next = state.clone();
    next.credential = Some(tokens.credential.clone());
    next.pending = None;
    store.save(&next)?;
    *state = next;
    Ok(tokens)
}

/// Complete a persisted enrollment or device approval without creating a new request identity.
/// # Errors
/// Preserves pending state on transport/storage failures; rejects denied or mismatched grants.
pub async fn complete(
    center: &impl Center,
    store: &impl CredentialStore,
    state: &mut State,
    cancel: &CancellationToken,
) -> Result<(), Error> {
    let mut retry_seconds = 5;
    loop {
        let result = match state.pending.as_ref().ok_or(Error::Protocol)? {
            Pending::Enrollment { token, request_id } => {
                center.enroll(token, *request_id, &state.machine).await
            }
            Pending::Web {
                device_code,
                request_id,
                expires_at,
                interval,
                ..
            } => {
                // A consumed code can outlive the approval deadline through its receipt.
                // Ask the center with the original id instead of discarding recovery locally.
                let seconds = if *expires_at <= Utc::now() {
                    0
                } else {
                    *interval
                };
                tokio::select! {
                    () = cancel.cancelled() => return Err(Error::Denied),
                    () = tokio::time::sleep(Duration::from_secs(seconds)) => {},
                }
                center.poll(device_code, *request_id).await
            }
            Pending::Refresh(_) => return Err(Error::Protocol),
        };
        match result {
            Ok(tokens) => {
                if tokens.credential.binding.server_id != state.machine.server_id {
                    return Err(Error::Protocol);
                }
                let mut next = state.clone();
                next.credential = Some(tokens.credential);
                next.pending = None;
                store.save(&next)?;
                *state = next;
                return Ok(());
            }
            Err(Error::Pending | Error::SlowDown) if matches!(state.pending, Some(Pending::Web { expires_at, .. }) if expires_at <= Utc::now()) =>
            {
                return Err(Error::Denied);
            }
            Err(Error::Pending) => {
                retry_seconds = 5;
                if matches!(state.pending, Some(Pending::Enrollment { .. })) {
                    wait(cancel, 5).await?;
                }
            }
            Err(Error::SlowDown) => {
                retry_seconds = 5;
                if let Some(Pending::Web { interval, .. }) = &mut state.pending {
                    *interval = (*interval + 5).min(300);
                    store.save(state)?;
                } else {
                    return Err(Error::Protocol);
                }
            }
            Err(Error::Unavailable) => {
                wait(cancel, retry_seconds).await?;
                retry_seconds = (retry_seconds * 2).min(60);
            }
            Err(error) => return Err(error),
        }
    }
}

async fn wait(cancel: &CancellationToken, seconds: u64) -> Result<(), Error> {
    tokio::select! {
        () = cancel.cancelled() => Err(Error::Denied),
        () = tokio::time::sleep(Duration::from_secs(seconds)) => Ok(()),
    }
}
