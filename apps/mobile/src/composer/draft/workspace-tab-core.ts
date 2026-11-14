import { resolveAgentCreationReadiness } from "@/provider-selection/provider-selection";

export interface WorkspaceDraftAutoSubmitConfig {
  provider: string;
  model: string | null;
}

export function validateDraftCreation(input: {
  composerState: {
    providerDefinitions: unknown[];
    selectedProvider: string | null;
    isModelLoading: boolean;
    effectiveModelId: string | null;
    availableModels: unknown[];
  };
  autoSubmitConfig: WorkspaceDraftAutoSubmitConfig | null;
  workspaceDirectory: string | null;
  hasClient: boolean;
}): string | null {
  const { composerState, autoSubmitConfig, workspaceDirectory, hasClient } = input;
  const readiness = resolveAgentCreationReadiness({
    providerCount: composerState.providerDefinitions.length,
    selection: {
      provider: composerState.selectedProvider,
      modelId: composerState.effectiveModelId ?? "",
      availableModels: composerState.availableModels,
      isModelLoading: composerState.isModelLoading,
    },
    autoSubmitConfig,
    workspaceDirectory,
    hasClient,
  });
  return readiness.ok ? null : (readiness.reason ?? null);
}

// Reconcile the form's selected mode against the currently discovered modes.
// The mode picker displays modeOptions[0] when the stored mode isn't in the
// list (e.g. a globally-remembered "plan" that this workspace's OpenCode config
// no longer defines), so the submitted mode must match that display — otherwise
// we'd send a stale mode the provider rejects while the UI showed a valid one.
function reconcileSelectedMode(modeOptionIds: readonly string[], selectedMode: string): string {
  if (modeOptionIds.length === 0) {
    return "";
  }
  return modeOptionIds.includes(selectedMode) ? selectedMode : (modeOptionIds[0] ?? "");
}

/**
 * Mode a draft agent is created with. The create request and the optimistic draft agent both
 * use it, so the draft shows the mode the created agent gets. An auto-submit setup that names
 * no mode uses the form's.
 */
export function resolveDraftModeId(input: {
  autoSubmitConfig: { modeId: string | null } | null;
  modeOptionIds: readonly string[];
  selectedMode: string;
}): string | null {
  const { autoSubmitConfig, modeOptionIds, selectedMode } = input;
  if (autoSubmitConfig?.modeId) {
    return autoSubmitConfig.modeId;
  }
  const reconciled = reconcileSelectedMode(modeOptionIds, selectedMode);
  if (reconciled !== "") {
    return reconciled;
  }
  return null;
}
