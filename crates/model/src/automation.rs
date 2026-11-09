//! Hub automation: the Ait center relays chat messages (Feishu, Slack, Telegram)
//! to Workspace Bots through its own Hub sessions.
//!
//! The owner opts in per Bot by enabling its Hub channel. The daemon publishes the
//! Bots the center may reach with [`PUBLISH_METHOD`], and the center opens
//! automation sessions only to a serve link whose latest publication is enabled.

/// Center method that publishes the Bots this Host lets the center reach.
pub const PUBLISH_METHOD: &str = "hub.automation.publish";

/// Version of the publication body.
pub const PUBLICATION_VERSION: u32 = 2;

/// A publication lists at most this many Bots, matching the center.
pub const MAX_BOTS: usize = 64;
