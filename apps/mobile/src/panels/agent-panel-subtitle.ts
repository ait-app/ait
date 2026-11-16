import { i18n } from "@/i18n/i18next";

function formatProviderLabel(provider: string): string {
  return provider
    .split(/[-_\s]+/)
    .filter((part) => part.length > 0)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

/**
 * "Codex agent" for a named provider. Without a provider name the subtitle is the plain fallback
 * ("Agent"), not the fallback dropped into the "{{provider}} agent" template.
 */
export function formatAgentPanelSubtitle(provider: string): string {
  const providerLabel = formatProviderLabel(provider);
  if (!providerLabel) {
    return i18n.t("workspace.tabs.fallback.agent");
  }
  return i18n.t("panels.agent.subtitle", { provider: providerLabel });
}

/** "Codex subagent", or the plain "Subagent" when there is no provider name to show. */
export function formatProviderSubagentSubtitle(provider: string): string {
  const providerLabel = formatProviderLabel(provider);
  if (!providerLabel) {
    return i18n.t("panels.providerSubagent.label");
  }
  return i18n.t("panels.providerSubagent.subtitle", { provider: providerLabel });
}
