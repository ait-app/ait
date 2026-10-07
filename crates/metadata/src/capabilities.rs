//! Connection method declarations and metadata service installation.

use model::methods::MethodSpec;

const CONNECTION_METHODS: &[MethodSpec] = &[
    MethodSpec::request("server.info"),
    MethodSpec::request("connection.ping"),
    MethodSpec::request("server.status.subscribe"),
    MethodSpec::request("subscription.release.request"),
    MethodSpec::request("editor.available.list.request"),
    MethodSpec::request("editor.open.request"),
    MethodSpec::event("session.heartbeat"),
    MethodSpec::request("session.events.set_subscription.request"),
    MethodSpec::request("creation.subscribe.request"),
];

/// Return the connection methods available independently of metadata service installation.
/// # Returns
/// Static names and message directions for host negotiation and envelope validation.
pub fn connection_methods() -> impl Iterator<Item = MethodSpec> {
    CONNECTION_METHODS.iter().copied()
}

/// Return every optional metadata service method for negotiation and envelope validation.
/// # Returns
/// Static names and message directions, independent of host installation or backend availability.
pub fn implemented_methods() -> impl Iterator<Item = MethodSpec> {
    [
        crate::connection::push::METHODS,
        crate::rpc::directory::METHODS,
        crate::rpc::directory::PROJECT_CONFIG_METHODS,
        crate::rpc::directory::PROJECT_ICON_METHODS,
        crate::connection::daemon::METHODS,
        crate::rpc::workspace_labels::METHODS,
        crate::rpc::workspace_automation::METHODS,
        crate::rpc::workspace_state::METHODS,
    ]
    .into_iter()
    .flat_map(|methods| methods.iter().copied())
}

/// Return the complete metadata service method set when its service is `installed`.
/// # Arguments
/// * `installed` - Whether the host provides this crate's optional business service.
/// # Returns
/// Every business method for a present service, or an empty iterator for an absent service.
/// Always available connection methods are returned separately by [`connection_methods`].
pub fn installed_methods(installed: bool) -> impl Iterator<Item = MethodSpec> {
    implemented_methods().filter(move |_| installed)
}

#[cfg(test)]
mod tests;
