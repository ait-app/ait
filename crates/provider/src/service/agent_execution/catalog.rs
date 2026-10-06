//! Catalog methods share bounded transport admission and independent request execution.

/// Select catalog methods without routing Agent or native-session lifecycle commands here.
pub(super) fn handles(method: &str) -> bool {
    matches!(
        method,
        "provider.available.list.request"
            | "provider.models.list.request"
            | "provider.modes.list.request"
            | "provider.features.list.request"
            | "provider.snapshot.get.request"
            | "provider.snapshot.refresh.request"
    )
}
