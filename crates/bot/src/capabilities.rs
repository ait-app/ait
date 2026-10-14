//! Bot method declarations and installation-based capability discovery.

use model::methods::MethodSpec;

pub(crate) const METHODS: &[MethodSpec] = &[
    MethodSpec::request("bot.create.request"),
    MethodSpec::request("bot.list.request"),
    MethodSpec::request("bot.get.request"),
    MethodSpec::request("bot.update.request"),
    MethodSpec::request("bot.archive.request"),
    MethodSpec::request("bot.channel.list.request"),
    MethodSpec::request("bot.channel.types.request"),
    MethodSpec::request("bot.channel.create.request"),
    MethodSpec::request("bot.channel.update.request"),
    MethodSpec::request("bot.channel.delete.request"),
    MethodSpec::request("bot.channel.status.request"),
    MethodSpec::request("bot.conversation.list.request"),
    MethodSpec::request("bot.conversation.open.request"),
    MethodSpec::request("bot.conversation.reset.request"),
    MethodSpec::request("bot.message.send.request"),
    MethodSpec::request("bot.history.get.request"),
    MethodSpec::request("bot.turn.get.request"),
    MethodSpec::request("bot.turn.cancel.request"),
    MethodSpec::request("bot.permission.resolve.request"),
    MethodSpec::request("bot.memory.get.request"),
    MethodSpec::request("bot.memory.update.request"),
];

/// Return every Bot-owned request method for envelope validation.
pub fn implemented_methods() -> impl Iterator<Item = MethodSpec> {
    METHODS.iter().copied()
}

/// Advertise Bot methods only when the host installs the complete Bot service.
pub fn installed_methods(installed: bool) -> impl Iterator<Item = MethodSpec> {
    implemented_methods().filter(move |_| installed)
}
