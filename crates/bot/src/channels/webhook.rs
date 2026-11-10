//! Webhook channels: the Ait center hosts an HTTP endpoint for the channel and relays each
//! call into it over the center's own Hub session, as a Webhook input.
//!
//! Creating one asks the center over this Host's serve link for the endpoint; the center
//! keeps the secret and returns it once, so the daemon stores only the endpoint and URL.

use std::sync::Arc;
use std::time::Duration;

use domain::ChannelId;
use domain::bot::ChannelConfig;
use model::ErrorCode;
use model::hub::{CallError, Center, Scope};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::protocol::{ChannelCreate, ChannelRequest, WebhookCreate};
use crate::service::Bots;

/// Center method that issues an endpoint, or replaces its secret, for one channel.
pub(crate) const CREATE_METHOD: &str = "hub.automation.webhook.create";
/// Center method that removes a channel's endpoint.
pub(crate) const DELETE_METHOD: &str = "hub.automation.webhook.delete";
/// A call is queued while the link reconnects; the owner should not wait longer than this.
const CALL_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) const FAILED_OFFLINE: &str = "centerOffline";
pub(crate) const FAILED_UNSUPPORTED: &str = "centerUnsupported";
pub(crate) const FAILED_UNAVAILABLE: &str = "automationUnavailable";
pub(crate) const FAILED_LIMIT: &str = "limitReached";
pub(crate) const FAILED_ERROR: &str = "centerError";

/// What the center issues for one channel.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Issued {
    endpoint_id: String,
    url: String,
    secret: String,
}

fn failed(reason: &str) -> Value {
    json!({"state":"failed","reason":reason})
}

/// The serve link's authority, when it is online and offers `method`.
fn link(center: Option<&Arc<dyn Center>>, method: &str) -> Result<Scope, &'static str> {
    let center = center.ok_or(FAILED_OFFLINE)?;
    let state = center.state().borrow().clone();
    let scope = state.scope.clone().filter(|_| state.online);
    let Some(scope) = scope else {
        return Err(FAILED_OFFLINE);
    };
    if !state.supports(method) {
        return Err(FAILED_UNSUPPORTED);
    }
    Ok(scope)
}

async fn call(
    center: &dyn Center,
    scope: Scope,
    method: &'static str,
    params: Value,
) -> Result<Value, &'static str> {
    match tokio::time::timeout(CALL_TIMEOUT, center.call(scope, method, params)).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(CallError::Unavailable)) | Err(_) => Err(FAILED_OFFLINE),
        Ok(Err(CallError::Unsupported)) => Err(FAILED_UNSUPPORTED),
        Ok(Err(CallError::Rejected(code))) => Err(match code.as_str() {
            "automation_unavailable" => FAILED_UNAVAILABLE,
            "limit_reached" => FAILED_LIMIT,
            _ => FAILED_ERROR,
        }),
        Ok(Err(CallError::Capacity | CallError::Protocol)) => Err(FAILED_ERROR),
    }
}

/// Ask the center for `channel`'s endpoint; a repeated call replaces the secret.
async fn issue(
    center: &Arc<dyn Center>,
    workspace: &str,
    channel: ChannelId,
) -> Result<Issued, &'static str> {
    let scope = link(Some(center), CREATE_METHOD)?;
    let value = call(
        center.as_ref(),
        scope,
        CREATE_METHOD,
        json!({"workspaceId":workspace,"channelId":channel}),
    )
    .await?;
    let issued: Issued = serde_json::from_value(value).map_err(|_| FAILED_ERROR)?;
    if issued.secret.is_empty() {
        return Err(FAILED_ERROR);
    }
    Ok(issued)
}

/// Create an enabled webhook channel whose endpoint the center issues now.
/// The secret is returned once and never stored by the daemon.
pub(crate) async fn create(bots: &Bots, request: WebhookCreate) -> Result<Value, ErrorCode> {
    bots.check_channel_admission(request.workspace_id.clone())
        .await?;
    let Some(center) = bots.center() else {
        return Ok(failed(FAILED_OFFLINE));
    };
    let channel_id = ChannelId::generate();
    let issued = match issue(&center, &request.workspace_id, channel_id).await {
        Ok(issued) => issued,
        Err(reason) => {
            // A lost reply may still have created the endpoint.
            if reason == FAILED_OFFLINE || reason == FAILED_ERROR {
                tokio::spawn(release(bots.clone(), channel_id));
            }
            return Ok(failed(reason));
        }
    };
    let created = bots
        .create_webhook_channel(ChannelCreate {
            workspace_id: request.workspace_id,
            channel_id,
            config: ChannelConfig::Webhook {
                endpoint_id: issued.endpoint_id,
                url: issued.url,
            },
            enabled: true,
        })
        .await;
    match created {
        Ok(created) => Ok(json!({
            "state": "created",
            "channel": created["channel"],
            "secret": issued.secret,
        })),
        Err(error) => {
            tokio::spawn(release(bots.clone(), channel_id));
            Err(error)
        }
    }
}

/// Replace a webhook channel's secret; the old one stops working at the center.
pub(crate) async fn rotate(bots: &Bots, request: ChannelRequest) -> Result<Value, ErrorCode> {
    bots.check_webhook_channel(request.workspace_id.clone(), request.channel_id)
        .await?;
    let Some(center) = bots.center() else {
        return Ok(failed(FAILED_OFFLINE));
    };
    match issue(&center, &request.workspace_id, request.channel_id).await {
        Ok(issued) => Ok(json!({"state":"rotated","secret":issued.secret,"url":issued.url})),
        Err(reason) => Ok(failed(reason)),
    }
}

/// Remove a deleted channel's endpoint; best effort, since the center stops routing to
/// channels this Host no longer publishes.
pub(crate) async fn release(bots: Bots, channel: ChannelId) {
    let Some(center) = bots.center() else {
        return;
    };
    let Ok(scope) = link(Some(&center), DELETE_METHOD) else {
        tracing::info!(%channel, "Center unreachable; its webhook endpoint stays until removed there");
        return;
    };
    if let Err(reason) = call(
        center.as_ref(),
        scope,
        DELETE_METHOD,
        json!({"channelId":channel}),
    )
    .await
    {
        tracing::info!(%channel, reason, "Could not remove a deleted channel's webhook endpoint");
    }
}

#[cfg(test)]
mod tests;
