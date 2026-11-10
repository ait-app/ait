//! Default-deny request policy for automation sessions.
//!
//! The center may deliver text to enabled Webhook channels, read and cancel their
//! turns, acknowledge replies and close their conversations. The Bot's existing
//! permissions govern execution; this session cannot access other capabilities.

use domain::BotTurnId;
use model::{ErrorCode, Request};

use crate::Shared;

/// Bot methods that act only on the named Workspace's enabled Webhook channels.
const WEBHOOK_METHODS: [&str; 3] = [
    "bot.webhook.message.send.request",
    "bot.webhook.delivery.ack.request",
    "bot.webhook.conversation.close.request",
];

/// Turn methods shared with owner sessions; the turn must be in a Webhook channel.
const TURN_METHODS: [&str; 2] = ["bot.turn.get.request", "bot.turn.cancel.request"];

/// Admit `request` from an automation session.
///
/// # Errors
/// Returns `Forbidden` for methods and targets outside the automation policy, and
/// `InvalidMessage` for malformed arguments.
pub(crate) async fn request(state: &Shared, request: &Request) -> Result<(), ErrorCode> {
    let method = request.method.as_str();
    if WEBHOOK_METHODS.contains(&method) {
        // The Bot itself refuses anything but the Workspace's enabled Webhook channels.
        return Ok(());
    }
    if !TURN_METHODS.contains(&method) {
        return Err(ErrorCode::Forbidden);
    }
    let bots = state.bot.bots.as_ref().ok_or(ErrorCode::Forbidden)?;
    let workspace = request.params["workspaceId"]
        .as_str()
        .ok_or(ErrorCode::InvalidMessage)?;
    let turn: BotTurnId = serde_json::from_value(request.params["turnId"].clone())
        .map_err(|_| ErrorCode::InvalidMessage)?;
    bots.check_webhook_turn(workspace, turn).await
}

#[cfg(test)]
mod tests;
