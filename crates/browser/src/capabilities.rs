//! Browser capability ownership.
/// Crate-owned method group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// Browser operations.
    Browser,
}
/// Implemented methods recognized by this crate's request handler.
pub const IMPLEMENTED_GROUPS: &[(Group, &[&str])] =
    &[(Group::Browser, crate::protocol::CAPABILITIES)];
/// Installed methods when the host composes this service.
pub fn installed_capabilities(installed: bool) -> impl Iterator<Item = &'static str> {
    crate::protocol::CAPABILITIES
        .iter()
        .copied()
        .filter(move |_| installed)
}
