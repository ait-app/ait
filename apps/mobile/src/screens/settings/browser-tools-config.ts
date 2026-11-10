import type { MutableDaemonConfig } from "@ait/protocol/messages";
import { toErrorMessage } from "@/utils/error-messages";

export const BROWSER_TOOLS_TITLE_KEY = "settings.host.agents.browserTools.title";
export const BROWSER_TOOLS_WARNING_KEY = "settings.host.agents.browserTools.hint";

export interface BrowserToolsCardState {
  isVisible: boolean;
  isEnabled: boolean;
  titleKey: string;
  warningKey: string;
}

export interface BrowserToolsMutationViewState {
  isSwitchDisabled: boolean;
  loadingKey: string | null;
  errorText: string | null;
}

export function getBrowserToolsCardState(input: {
  isConnected: boolean;
  config: MutableDaemonConfig | null;
}): BrowserToolsCardState {
  return {
    isVisible: input.isConnected,
    isEnabled: input.config?.browserTools.enabled === true,
    titleKey: BROWSER_TOOLS_TITLE_KEY,
    warningKey: BROWSER_TOOLS_WARNING_KEY,
  };
}

export function createBrowserToolsPatch(enabled: boolean): Partial<MutableDaemonConfig> {
  return { browserTools: { enabled } };
}

export function getBrowserToolsMutationViewState(input: {
  isPending: boolean;
  error: unknown;
}): BrowserToolsMutationViewState {
  return {
    isSwitchDisabled: input.isPending,
    loadingKey: input.isPending ? "settings.host.agents.browserTools.updating" : null,
    errorText: input.error ? toErrorMessage(input.error) : null,
  };
}
