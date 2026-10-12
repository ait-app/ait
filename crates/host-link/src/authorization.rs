//! Durable credential restoration shared by login and unattended runtime.

use chrono::Utc;
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
