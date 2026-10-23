//! Catalog and native session-list reads share bounded transport response admission.

/// Select discovery and session enumeration without admitting session lifecycle mutations here.
pub(super) fn handles(method: &str) -> bool {
    matches!(
        method,
        "provider.available.list.request"
            | "provider.models.list.request"
            | "provider.modes.list.request"
            | "provider.features.list.request"
            | "provider.snapshot.get.request"
            | "provider.snapshot.refresh.request"
            | "provider.sessions.recent.list.request"
    )
}
