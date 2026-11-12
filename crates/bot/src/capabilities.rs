//! Bot method declarations and installation-based capability discovery.

use model::methods::MethodSpec;

pub(crate) const METHODS: &[MethodSpec] = &[
    MethodSpec::request("bot.get.request"),
    MethodSpec::request("bot.setup.request"),
    MethodSpec::request("bot.update.request"),
    MethodSpec::request("bot.permissions.update.request"),
    MethodSpec::request("bot.pause.request"),
    MethodSpec::request("bot.resume.request"),
    MethodSpec::request("bot.remove.request"),
    MethodSpec::request("bot.channel.list.request"),
    MethodSpec::request("bot.channel.types.request"),
    MethodSpec::request("bot.channel.create.request"),
    MethodSpec::request("bot.channel.update.request"),
    MethodSpec::request("bot.channel.delete.request"),
    MethodSpec::request("bot.channel.status.request"),
    MethodSpec::request("bot.channel.feishu.scan.start.request"),
    MethodSpec::request("bot.channel.feishu.scan.status.request"),
    MethodSpec::request("bot.channel.feishu.scan.cancel.request"),
    MethodSpec::request("bot.channel.feishu.connect.request"),
    MethodSpec::request("bot.channel.telegram.pair.start.request"),
    MethodSpec::request("bot.channel.telegram.pair.status.request"),
    MethodSpec::request("bot.channel.telegram.pair.cancel.request"),
    MethodSpec::request("bot.channel.weixin.scan.start.request"),
    MethodSpec::request("bot.channel.weixin.scan.status.request"),
    MethodSpec::request("bot.channel.weixin.scan.cancel.request"),
    MethodSpec::request("bot.channel.qq.scan.start.request"),
    MethodSpec::request("bot.channel.qq.scan.status.request"),
    MethodSpec::request("bot.channel.qq.scan.cancel.request"),
    MethodSpec::request("bot.channel.qq.connect.request"),
    MethodSpec::request("bot.channel.qq.pair.request"),
    MethodSpec::request("bot.channel.imessage.pair.start.request"),
    MethodSpec::request("bot.channel.imessage.pair.status.request"),
    MethodSpec::request("bot.channel.imessage.pair.cancel.request"),
    MethodSpec::request("bot.channel.webhook.create.request"),
    MethodSpec::request("bot.channel.webhook.rotate.request"),
    MethodSpec::request("bot.conversation.list.request"),
    MethodSpec::request("bot.conversation.open.request"),
    MethodSpec::request("bot.conversation.reset.request"),
    MethodSpec::request("bot.message.send.request"),
    MethodSpec::request("bot.history.get.request"),
    MethodSpec::request("bot.turn.get.request"),
    MethodSpec::request("bot.turn.cancel.request"),
    MethodSpec::request("bot.memory.get.request"),
    MethodSpec::request("bot.memory.update.request"),
    MethodSpec::request("bot.notifications.list.request"),
    MethodSpec::request("bot.webhook.message.send.request"),
    MethodSpec::request("bot.webhook.delivery.ack.request"),
    MethodSpec::request("bot.webhook.conversation.close.request"),
];

/// Return every Bot-owned request method for envelope validation.
pub fn implemented_methods() -> impl Iterator<Item = MethodSpec> {
    METHODS.iter().copied()
}

/// Advertise Bot methods only when the host installs the complete Bot service.
pub fn installed_methods(installed: bool) -> impl Iterator<Item = MethodSpec> {
    implemented_methods().filter(move |_| installed)
}
