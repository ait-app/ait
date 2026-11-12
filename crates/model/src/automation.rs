//! The Ait center hosts public endpoints for Workspace Bot automation channels.
//! The owner opts in by creating and enabling a Webhook or GitHub channel. The daemon
//! publishes those channels over its existing serve link; the center can then
//! open restricted sessions to deliver calls and retrieve their replies.

/// Center method that publishes the automation channels this Host lets the center reach.
pub const PUBLISH_METHOD: &str = "hub.automation.publish";

/// Version of the publication body.
pub const PUBLICATION_VERSION: u32 = 2;

/// A publication lists at most this many automation channels, matching the center.
pub const MAX_BOTS: usize = 64;
