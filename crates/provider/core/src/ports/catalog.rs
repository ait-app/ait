//! How the provider catalog presents an adapter beyond its discovered models and modes.

/// Display name, description and default mode an adapter adds to its provider snapshot entry.
///
/// Adapters without a presentation leave the entry with only the discovered fields, and
/// clients fall back to their own label for the provider id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogPresentation {
    /// Human-readable provider name, published as `label`.
    pub label: &'static str,
    /// One-line description of how Ait drives the provider, published as `description`.
    pub description: &'static str,
    /// Where the entry's `defaultModeId` comes from.
    pub default_mode: DefaultMode,
}

/// Source of a snapshot entry's `defaultModeId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultMode {
    /// The entry carries no `defaultModeId` field.
    Omitted,
    /// The entry always names this mode id.
    Fixed(&'static str),
    /// The discovered mode flagged `isDefault`, or `null` when discovery flags none.
    Discovered,
}
