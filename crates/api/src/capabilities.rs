use browser::capabilities as browser;
use filesystem::capabilities as filesystem;
use metadata::capabilities as metadata;
use provider::capabilities as provider;
use schedule::capabilities as schedule;
use terminal::capabilities as terminal;
use voice::capabilities as voice;

use crate::Services;

/// Merge crate-owned method declarations for negotiation and envelope validation.
pub(super) fn implemented_methods() -> impl Iterator<Item = model::methods::MethodSpec> {
    schedule::implemented_methods()
        .chain(browser::implemented_methods())
        .chain(voice::implemented_methods())
        .chain(crate::core_methods::METHODS.iter().copied())
        .chain(metadata::implemented_methods())
        .chain(filesystem::implemented_methods())
        .chain(provider::implemented_methods())
        .chain(terminal::implemented_methods())
        .chain(crate::relay_rpc::METHODS.iter().copied())
}

/// Supply crate-level installation presence to each owner and collect its installed method names.
pub(super) fn installed_capabilities(services: &Services) -> Vec<String> {
    crate::core_methods::METHODS
        .iter()
        .copied()
        .chain(metadata::installed_methods(services.metadata.is_some()))
        .chain(filesystem::installed_methods(services.filesystem.is_some()))
        .chain(provider::installed_methods(services.provider.is_some()))
        .chain(terminal::installed_methods(services.terminal.is_some()))
        .chain(voice::installed_methods(services.voice.is_some()))
        .chain(schedule::installed_methods(services.schedule.is_some()))
        .chain(browser::installed_methods(services.browser.is_some()))
        .chain(crate::relay_rpc::METHODS.iter().copied())
        .map(|method| method.name.to_owned())
        .collect()
}

/// Behaviors that need versioned discovery even when their method names already existed.
pub(super) fn features(services: &Services) -> Vec<String> {
    let mut features = vec![protocol::single::FEATURE.to_owned()];
    if services.filesystem.is_some() && services.metadata.is_some() {
        features.push("checkout-git-events-v1".to_owned());
    }
    if services.filesystem.as_ref().is_some_and(|filesystem| {
        filesystem
            .dependencies()
            .forge
            .providers()
            .contains(&"gitlab")
    }) {
        features.push("forge-gitlab-v1".to_owned());
    }
    if services.metadata.is_some() && services.provider.is_some() {
        features.extend(
            ["directory-sync-v1", "directory-subscriptions-v1"]
                .into_iter()
                .map(str::to_owned),
        );
    }
    if services.terminal.is_some() {
        features.push("terminal-activity-v1".to_owned());
    }
    if services.provider.is_some() {
        features.push("agent-session-events-v1".to_owned());
        if services.metadata.is_some() {
            features.push("creation-lifecycle-v1".to_owned());
        }
    }
    features
}

#[cfg(test)]
mod tests;
