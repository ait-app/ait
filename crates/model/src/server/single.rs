//! Negotiation identifiers for one physical socket carrying all daemon capabilities.

/// Required hello capability that selects the four internal connection workers.
pub const CAPABILITY: &str = "connection.single.v1";

/// Server feature advertising support for the single-connection protocol.
pub const FEATURE: &str = "ait-rust-single-v1";

/// Maximum number of distinct capabilities offered by a single-connection client.
///
/// Clients offer the whole advertised catalogue, so this budget must stay above the installed
/// method count; ADR-282 records why it grew past the catalogue size.
pub const MAX_CAPABILITIES: usize = 512;
