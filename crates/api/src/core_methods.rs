//! Always-installed API connection methods, independent of business service installation.

use model::methods::MethodSpec;

pub(super) const METHODS: &[MethodSpec] = &[
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
