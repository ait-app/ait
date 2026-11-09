//! Default-deny request policy for automation sessions.
//!
//! The center may post text into a Workspace Bot's enabled center channels (its Hub
//! channel and webhook channels), read and cancel those channels' turns, acknowledge
//! their replies and close their conversations, and nothing else: no Agents, terminals, files, settings, shares,
//! other channels or daemon management. The Bot's own permissions bound what it
//! does with that text.

use domain::BotTurnId;
use model::{ErrorCode, Request};

use crate::Shared;

/// Bot methods that act only on the named Workspace's enabled center channels.
const HUB_METHODS: [&str; 3] = [
    "bot.hub.message.send.request",
    "bot.hub.delivery.ack.request",
    "bot.hub.conversation.close.request",
];

/// Turn methods shared with owner sessions; the turn must be in a center channel.
const TURN_METHODS: [&str; 2] = ["bot.turn.get.request", "bot.turn.cancel.request"];

/// Admit `request` from an automation session.
///
/// # Errors
/// Returns `Forbidden` for methods and targets outside the automation policy, and
/// `InvalidMessage` for malformed arguments.
pub(crate) async fn request(state: &Shared, request: &Request) -> Result<(), ErrorCode> {
    let method = request.method.as_str();
    if HUB_METHODS.contains(&method) {
        // The Bot itself refuses anything but the Workspace's enabled center channels.
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
    bots.check_hub_turn(workspace, turn).await
}

#[cfg(test)]
mod tests;
