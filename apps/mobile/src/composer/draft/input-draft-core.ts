import type { UserComposerAttachment } from "@/attachments/types";
import type { DraftAgentControlsProps } from "@/composer/agent-controls";
import type { UseAgentFormStateResult } from "@/hooks/use-agent-form-state";

export interface DraftKeyContext {
  selectedServerId: string | null;
}

export type DraftKeyInput = string | ((context: DraftKeyContext) => string);

export function resolveDraftKey(input: {
  draftKey: DraftKeyInput;
  selectedServerId: string | null;
}): string {
  if (typeof input.draftKey === "function") {
    return input.draftKey({ selectedServerId: input.selectedServerId });
  }
  return input.draftKey;
}

export function buildDraftAgentControls(input: {
  formState: UseAgentFormStateResult;
  features?: DraftAgentControlsProps["features"];
  onSetFeature?: DraftAgentControlsProps["onSetFeature"];
  onApplyAgentProfile: DraftAgentControlsProps["onApplyAgentProfile"];
  onApplyModelPreset?: DraftAgentControlsProps["onApplyModelPreset"];
}): DraftAgentControlsProps {
  const { formState, features, onSetFeature, onApplyAgentProfile, onApplyModelPreset } = input;
  return {
    providerDefinitions: formState.providerDefinitions,
    selectedProvider: formState.selectedProvider,
    modeOptions: formState.modeOptions,
    selectedMode: formState.selectedMode,
    onSelectMode: formState.setModeFromUser,
    selectedModel: formState.selectedModel,
    onSelectModel: formState.setModelFromUser,
    modelSelectorProviders: formState.modelSelectorProviders,
    isAllModelsLoading: formState.isAllModelsLoading,
    onSelectProviderAndModel: formState.setProviderAndModelFromUser,
    thinkingOptions: formState.availableThinkingOptions,
    selectedThinkingOptionId: formState.selectedThinkingOptionId,
    onSelectThinkingOption: formState.setThinkingOptionFromUser,
    onApplyAgentProfile,
    onApplyModelPreset,
    modelSelectorCwd: formState.workingDir,
    features,
    onSetFeature,
    onModelSelectorOpen: formState.refetchProviderModelsIfStale,
    onRetryModelProvider: formState.refreshProviderModels,
    isRetryingModelProvider: formState.isProviderModelsRefreshing,
    modelSelectorServerId: formState.selectedServerId,
  };
}

export function hasDraftContent(input: {
  text: string;
  attachments: UserComposerAttachment[];
}): boolean {
  return input.text.length > 0 || input.attachments.length > 0;
}
