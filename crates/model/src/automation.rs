//! Hub automation: the Ait center relays chat messages (Feishu, Slack, Telegram) and
//! calls to the webhooks it hosts to Workspace Bots through its own Hub sessions.
//!
//! The owner opts in per Bot by enabling its Hub channel or creating a webhook
//! channel. The daemon publishes those channels with [`PUBLISH_METHOD`], one entry
//! each, and the center opens automation sessions only to a serve link whose latest
//! publication is enabled.

/// Center method that publishes the center channels this Host lets the center reach.
pub const PUBLISH_METHOD: &str = "hub.automation.publish";

/// Version of the publication body.
pub const PUBLICATION_VERSION: u32 = 2;

/// A publication lists at most this many center channels, matching the center.
pub const MAX_BOTS: usize = 64;
