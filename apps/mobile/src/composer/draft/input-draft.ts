import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { UserComposerAttachment } from "@/attachments/types";
import type { TextReplacement } from "@/composer/types";
import type { DraftAgentControlsProps } from "@/composer/agent-controls";
import type { DraftCommandConfig } from "@/hooks/use-agent-commands-query";
import {
  useAgentFormState,
  type CreateAgentInitialValues,
  type UseAgentFormStateResult,
} from "@/hooks/use-agent-form-state";
import {
  mergeModelPresetFeatureValues,
  isPresetOwnedFeature,
  modelPresetConfiguration,
  withModelPresetRoleDefaults,
  type ModelPresetConfiguration,
  type ModelPresetControls,
} from "@/agent-controls/model-presets";
import { useDraftAgentFeatures } from "@/hooks/use-draft-agent-features";
import { useFormPreferences } from "@/hooks/use-form-preferences";
import {
  buildDraftAgentControls,
  hasDraftContent,
  resolveDraftKey,
  type DraftKeyInput,
} from "@/composer/draft/input-draft-core";
import {
  buildDraftCommandConfig,
  resolveEffectiveComposerModelId,
  resolveEffectiveComposerThinkingOptionId,
  type ProviderSelectionState,
} from "@/provider-selection/provider-selection";
import {
  registerDraftEditorPublication,
  subscribeToSharedDraftState,
  useDraftStore,
} from "@/stores/draft-store";
import { AfterPaintPublication } from "@/composer/after-paint-publication";
import { useShallow } from "zustand/shallow";
import type { ComposerTextSource } from "@/composer/text-source";
import { isWeb } from "@/constants/platform";
import { useSessionStore } from "@/stores/session-store";
import type { ModelPresetSelection } from "@ait/protocol/messages";
import type { DraftModelPresetSelection } from "@/stores/draft-store/state";
import { useProviderPresets } from "@/data/provider-presets";
import { usePresetPermissionDenied } from "@/agent-controls/preset-permission";

type AttachmentUpdater =
  | UserComposerAttachment[]
  | ((prev: UserComposerAttachment[]) => UserComposerAttachment[]);

interface AgentInputDraftComposerOptions {
  initialServerId: string | null;
  initialValues?: CreateAgentInitialValues;
  initialFeatureValues?: Record<string, unknown>;
  isVisible?: boolean;
  lockedWorkingDir?: string;
}

interface UseAgentInputDraftInput {
  draftKey: DraftKeyInput;
  composer?: AgentInputDraftComposerOptions;
}

type DraftComposerState = UseAgentFormStateResult & {
  workingDir: string;
  effectiveModelId: string;
  effectiveThinkingOptionId: string;
  featureValues: Record<string, unknown> | undefined;
  modelPreset:
    | (ModelPresetSelection & {
        baseFeatureValues: Record<string, unknown>;
        baseThinkingOptionId: string | null;
      })
    | null;
  agentControls: DraftAgentControlsProps;
  commandDraftConfig: DraftCommandConfig | undefined;
};

export interface AgentInputDraft {
  textSource: ComposerTextSource;
  editText: (text: string) => void;
  replaceText: (text: string) => void;
  textReplacement: TextReplacement;
  attachments: UserComposerAttachment[];
  setAttachments: (updater: AttachmentUpdater) => void;
  clear: (lifecycle: "sent" | "abandoned") => void;
  isHydrated: boolean;
  attachmentFocusRequestId: number;
  composerState: DraftComposerState | null;
}

export function useAgentInputDraft(input: UseAgentInputDraftInput): AgentInputDraft {
  const composerOptions = input.composer ?? null;
  const { preferences } = useFormPreferences();
  const workingDir = composerOptions?.lockedWorkingDir?.trim() || "";
  const formState = useAgentFormState({
    workingDir,
    serverId: composerOptions?.initialServerId ?? null,
    initialValues: composerOptions?.initialValues,
    isVisible: composerOptions?.isVisible ?? false,
    isCreateFlow: true,
  });
  const draftKey = useMemo(
    () =>
      resolveDraftKey({
        draftKey: input.draftKey,
        selectedServerId: formState.selectedServerId,
      }),
    [formState.selectedServerId, input.draftKey],
  );
  const attachments = useDraftStore(
    useShallow((state) =>
      state.drafts[draftKey]?.lifecycle === "active"
        ? (state.drafts[draftKey].input.attachments ?? [])
        : [],
    ),
  );
  const textSource = useMemo<ComposerTextSource>(
    () => ({
      getSnapshot: () => {
        const record = useDraftStore.getState().drafts[draftKey];
        return record?.lifecycle === "active" ? record.input.text : "";
      },
      subscribe: (listener) =>
        useDraftStore.subscribe((state, previous) => {
          if (
            state.drafts[draftKey]?.input.text !== previous.drafts[draftKey]?.input.text ||
            state.drafts[draftKey]?.lifecycle !== previous.drafts[draftKey]?.lifecycle
          )
            listener();
        }),
    }),
    [draftKey],
  );
  const attachmentFocusRequestId = useDraftStore(
    (state) => state.attachmentFocusRequestByDraftKey[draftKey] ?? 0,
  );
  const [hydratedDraftKey, setHydratedDraftKey] = useState<string | null>(null);
  const isHydrated = hydratedDraftKey === draftKey;
  const textReplacementRevisionRef = useRef(0);
  const [textReplacement, setTextReplacement] = useState<TextReplacement>(() => ({
    kind: "initial",
    key: `${draftKey}:0`,
    text: textSource.getSnapshot(),
  }));

  const publishTextReplacement = useCallback(
    (kind: "hydrated" | "replace", nextText: string) => {
      textReplacementRevisionRef.current += 1;
      setTextReplacement({
        kind,
        key: `${draftKey}:${textReplacementRevisionRef.current}`,
        text: nextText,
      });
    },
    [draftKey],
  );

  const saveDraft = useCallback(
    (
      update: (draft: { text: string; attachments: UserComposerAttachment[] }) => {
        text: string;
        attachments: UserComposerAttachment[];
      },
    ) => {
      const store = useDraftStore.getState();
      const current = store.getDraftInput(draftKey) ?? { text: "", attachments: [] };
      const next = update(current);
      if (!hasDraftContent(next)) {
        store.clearDraftInput({ draftKey, lifecycle: "abandoned" });
        return;
      }
      store.saveDraftInput({ draftKey, draft: next });
    },
    [draftKey],
  );

  const textPublication = useMemo(
    () =>
      new AfterPaintPublication<string>((nextText) => {
        useDraftStore.getState().editDraftText({ draftKey, text: nextText });
      }),
    [draftKey],
  );

  useEffect(() => registerDraftEditorPublication(() => textPublication.flush()), [textPublication]);
  useEffect(
    () =>
      subscribeToSharedDraftState((previous, next) => {
        const before = previous.drafts[draftKey];
        const after = next.drafts[draftKey];
        const previousText = before?.lifecycle === "active" ? before.input.text : "";
        const nextText = after?.lifecycle === "active" ? after.input.text : "";
        if (previousText !== nextText) publishTextReplacement("replace", nextText);
      }),
    [draftKey, publishTextReplacement],
  );

  const editText = useCallback(
    (nextText: string) => {
      if (isWeb) {
        textPublication.stage(nextText);
      } else {
        useDraftStore.getState().editDraftText({ draftKey, text: nextText });
      }
    },
    [draftKey, textPublication],
  );

  const replaceText = useCallback(
    (nextText: string) => {
      textPublication.cancel();
      useDraftStore.getState().editDraftText({ draftKey, text: nextText });
      publishTextReplacement("replace", nextText);
    },
    [draftKey, publishTextReplacement, textPublication],
  );

  const setAttachments = useCallback(
    (updater: AttachmentUpdater) => {
      saveDraft((current) => ({
        ...current,
        attachments: typeof updater === "function" ? updater(current.attachments) : updater,
      }));
    },
    [saveDraft],
  );

  const clear = useCallback(
    (lifecycle: "sent" | "abandoned") => {
      textPublication.cancel();
      useDraftStore.getState().clearDraftInput({ draftKey, lifecycle });
    },
    [draftKey, textPublication],
  );

  useEffect(() => {
    const flushWhenHidden = () => {
      if (document.visibilityState === "hidden") textPublication.flush();
    };
    const flush = () => textPublication.flush();
    const canListenForPageHide =
      isWeb && typeof window !== "undefined" && typeof window.addEventListener === "function";
    if (isWeb && typeof document !== "undefined") {
      document.addEventListener("visibilitychange", flushWhenHidden);
    }
    if (canListenForPageHide) {
      window.addEventListener("pagehide", flush);
    }
    return () => {
      if (isWeb && typeof document !== "undefined") {
        document.removeEventListener("visibilitychange", flushWhenHidden);
      }
      if (canListenForPageHide) {
        window.removeEventListener("pagehide", flush);
      }
      textPublication.flush();
    };
  }, [textPublication]);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      await useDraftStore.getState().hydrateDraftInput({ draftKey });
      if (!cancelled) {
        const hydratedText = useDraftStore.getState().getDraftInput(draftKey)?.text ?? "";
        publishTextReplacement("hydrated", hydratedText);
        setHydratedDraftKey(draftKey);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [draftKey, publishTextReplacement]);

  const providerSelection = useMemo<ProviderSelectionState>(
    () => ({
      provider: formState.selectedProvider,
      modelId: formState.selectedModel,
      modeId: formState.selectedMode,
      thinkingOptionId: formState.selectedThinkingOptionId,
      availableModels: formState.availableModels,
      modeOptions: formState.modeOptions,
    }),
    [
      formState.availableModels.map((model) => ({
        id: model.id,
        aliases: model.aliases,
        thinkingOptions: model.thinkingOptions,
        defaultThinkingOptionId: model.defaultThinkingOptionId,
      })),
      formState.modeOptions,
      formState.selectedMode,
      formState.selectedModel,
      formState.selectedProvider,
      formState.selectedThinkingOptionId,
    ],
  );

  const effectiveModelId = useMemo(
    () => resolveEffectiveComposerModelId(providerSelection),
    [providerSelection],
  );

  const effectiveThinkingOptionId = useMemo(
    () => resolveEffectiveComposerThinkingOptionId(providerSelection, effectiveModelId),
    [effectiveModelId, providerSelection],
  );

  const {
    features: draftFeatures,
    featureValues: draftFeatureValues,
    setFeatureValue: setDraftFeatureValue,
    applyProfileFeatureValues,
  } = useDraftAgentFeatures({
    serverId: formState.selectedServerId,
    provider: formState.selectedProvider,
    cwd: workingDir,
    modeId: formState.selectedMode,
    modelId: effectiveModelId,
    thinkingOptionId: effectiveThinkingOptionId,
    initialFeatureValues: composerOptions?.initialFeatureValues,
  });

  const draftPreset = useDraftStore((state) =>
    state.drafts[draftKey]?.lifecycle === "active"
      ? (state.drafts[draftKey].input.modelPreset ?? null)
      : null,
  );
  const setDraftPreset = useCallback(
    (preset: DraftModelPresetSelection | null) => {
      const store = useDraftStore.getState();
      const current = store.getDraftInput(draftKey) ?? { text: "", attachments: [] };
      const { modelPreset: _old, ...rest } = current;
      const next = preset ? { ...rest, modelPreset: preset } : rest;
      if (hasDraftContent(next)) store.saveDraftInput({ draftKey, draft: next });
      else store.clearDraftInput({ draftKey, lifecycle: "abandoned" });
    },
    [draftKey],
  );
  const supportsModelPresetSelection = useSessionStore(
    (state) =>
      state.sessions[formState.selectedServerId ?? ""]?.serverInfo?.features
        ?.agentModelPresetSelection === true,
  );
  const presetPermissionDenied = usePresetPermissionDenied(formState.selectedServerId);
  const storedPreset = useProviderPresets({
    serverId: formState.selectedServerId,
    provider: draftPreset?.provider,
    cwd: draftPreset?.cwd,
    enabled: Boolean(draftPreset),
  });
  const restoredPresetRef = useRef<string | null>(null);
  useEffect(() => {
    if (!draftPreset || !storedPreset.snapshot || draftPreset.cwd !== workingDir) {
      restoredPresetRef.current = null;
      return;
    }
    const preset = storedPreset.snapshot.presets.find(
      (entry) => entry.name === draftPreset.name && entry.source === draftPreset.source,
    );
    if (
      !preset ||
      !formState.modelSelectorProviders.some((entry) => entry.id === draftPreset.provider)
    )
      return;
    if (formState.selectedProvider !== draftPreset.provider) {
      formState.applyModelPresetFromUser(
        draftPreset.provider,
        modelPresetConfiguration(preset, []),
      );
      return;
    }
    if (formState.isModelLoading) return;
    const key = JSON.stringify([
      draftKey,
      draftPreset,
      preset,
      (draftFeatures ?? []).map((feature) =>
        feature.type === "select"
          ? { id: feature.id, options: feature.options }
          : { id: feature.id, type: feature.type },
      ),
      formState.availableModels,
    ]);
    if (restoredPresetRef.current === key) return;
    restoredPresetRef.current = key;
    const complete = modelPresetConfiguration(
      preset,
      draftFeatures ?? [],
      formState.availableModels,
    );
    formState.applyModelPresetFromUser(draftPreset.provider, complete);
    applyProfileFeatureValues(
      mergeModelPresetFeatureValues(draftFeatureValues, complete),
      draftPreset.provider,
    );
  }, [
    draftKey,
    draftPreset,
    storedPreset.snapshot,
    workingDir,
    formState,
    draftFeatures,
    draftFeatureValues,
    applyProfileFeatureValues,
  ]);
  const exitDraftPreset = useCallback(() => {
    if (!draftPreset) return;
    applyProfileFeatureValues(
      mergeModelPresetFeatureValues(draftFeatureValues, {
        modelId: formState.selectedModel,
        thinkingOptionId: formState.selectedThinkingOptionId || null,
        featureValues: draftPreset.baseFeatureValues,
      }),
      draftPreset.provider,
    );
    setDraftPreset(null);
  }, [
    applyProfileFeatureValues,
    draftFeatureValues,
    draftPreset,
    formState.selectedModel,
    formState.selectedThinkingOptionId,
    setDraftPreset,
  ]);
  useEffect(() => {
    if (!draftPreset || !workingDir || draftPreset.cwd === workingDir) return;
    if (draftPreset.provider === formState.selectedProvider) {
      formState.applyModelPresetFromUser(draftPreset.provider, {
        modelId: effectiveModelId,
        thinkingOptionId:
          draftPreset.baseModelId === effectiveModelId ? draftPreset.baseThinkingOptionId : null,
        featureValues: {},
      });
    }
    exitDraftPreset();
  }, [draftPreset, effectiveModelId, exitDraftPreset, formState, workingDir]);

  const applyDraftAgentProfile = useCallback(
    (profile: Parameters<typeof formState.applyProfileFromUser>[0]) => {
      setDraftPreset(null);
      formState.applyProfileFromUser(profile);
      applyProfileFeatureValues(profile.featureValues);
    },
    [applyProfileFeatureValues, formState, setDraftPreset],
  );

  const applyDraftModelPreset = useCallback(
    (
      provider: string,
      configuration: ModelPresetConfiguration,
      preset: Parameters<ModelPresetControls["apply"]>[2],
    ) => {
      if (!supportsModelPresetSelection || presetPermissionDenied || !preset.source) return;
      const targetPreferences = preferences.providerPreferences?.[provider];
      const priorModel =
        provider === formState.selectedProvider
          ? effectiveModelId
          : (targetPreferences?.model ?? "");
      const baseFeatureValues =
        draftPreset?.provider === provider
          ? draftPreset.baseFeatureValues
          : Object.fromEntries(
              Object.entries(
                provider === formState.selectedProvider
                  ? (draftFeatureValues ?? {})
                  : (targetPreferences?.featureValues ?? {}),
              ).filter(([key]) => isPresetOwnedFeature(key)),
            );
      setDraftPreset({
        provider,
        cwd: workingDir,
        name: preset.name,
        source: preset.source,
        baseFeatureValues,
        baseModelId:
          draftPreset?.provider === provider
            ? draftPreset.baseModelId
            : provider === formState.selectedProvider
              ? effectiveModelId
              : priorModel,
        baseThinkingOptionId:
          draftPreset?.provider === provider
            ? draftPreset.baseThinkingOptionId
            : provider === formState.selectedProvider
              ? formState.selectedThinkingOptionId || null
              : priorModel
                ? (targetPreferences?.thinkingByModel?.[priorModel] ?? null)
                : null,
      });
      const complete =
        provider === formState.selectedProvider
          ? withModelPresetRoleDefaults(configuration, draftFeatures)
          : configuration;
      formState.applyModelPresetFromUser(provider, complete);
      applyProfileFeatureValues(
        mergeModelPresetFeatureValues(
          provider === formState.selectedProvider ? draftFeatureValues : undefined,
          complete,
        ),
        provider,
      );
    },
    [
      applyProfileFeatureValues,
      draftFeatureValues,
      draftFeatures,
      draftPreset,
      formState,
      supportsModelPresetSelection,
      presetPermissionDenied,
      setDraftPreset,
      workingDir,
      effectiveModelId,
      preferences.providerPreferences,
    ],
  );

  const commandDraftConfig = useMemo(
    () =>
      composerOptions
        ? buildDraftCommandConfig({
            selection: providerSelection,
            cwd: workingDir,
            effectiveModelId,
            effectiveThinkingOptionId,
            featureValues: draftFeatureValues,
          })
        : undefined,
    [
      composerOptions,
      effectiveModelId,
      effectiveThinkingOptionId,
      draftFeatureValues,
      providerSelection,
      workingDir,
    ],
  );

  const composerState = useMemo<DraftComposerState | null>(() => {
    if (!composerOptions) {
      return null;
    }

    return {
      ...formState,
      persistFormPreferences: draftPreset ? async () => {} : formState.persistFormPreferences,
      workingDir,
      effectiveModelId,
      effectiveThinkingOptionId,
      featureValues: draftFeatureValues,
      modelPreset: draftPreset
        ? {
            name: draftPreset.name,
            source: draftPreset.source,
            baseFeatureValues: draftPreset.baseFeatureValues,
            baseThinkingOptionId: draftPreset.baseThinkingOptionId,
          }
        : null,
      agentControls: {
        ...buildDraftAgentControls({
          formState,
          features: draftFeatures,
          onSetFeature: setDraftFeatureValue,
          onApplyAgentProfile: applyDraftAgentProfile,
          onApplyModelPreset: applyDraftModelPreset,
        }),
        selectedModelPreset:
          supportsModelPresetSelection && !presetPermissionDenied ? draftPreset : null,
        modelPresetPending: Boolean(
          draftPreset && (!supportsModelPresetSelection || presetPermissionDenied),
        ),
        supportsModelPresetSelection,
        presetPermissionDenied,
        onSelectModel: (modelId) => {
          const exit = draftPreset
            ? draftPreset.provider === formState.selectedProvider &&
              draftPreset.baseModelId === modelId
              ? { baselineThinking: draftPreset.baseThinkingOptionId }
              : {}
            : undefined;
          exitDraftPreset();
          formState.setModelFromUser(modelId, exit);
        },
        onSelectProviderAndModel: (provider, modelId) => {
          const exit = draftPreset
            ? draftPreset.provider === provider && draftPreset.baseModelId === modelId
              ? { baselineThinking: draftPreset.baseThinkingOptionId }
              : {}
            : undefined;
          exitDraftPreset();
          formState.setProviderAndModelFromUser(provider, modelId, exit);
        },
        onSelectThinkingOption: (id) => {
          if (!draftPreset) formState.setThinkingOptionFromUser(id);
        },
        onSetFeature: (id, value) => {
          if (!draftPreset || !isPresetOwnedFeature(id)) setDraftFeatureValue(id, value);
        },
      },
      commandDraftConfig,
    };
  }, [
    commandDraftConfig,
    composerOptions,
    effectiveModelId,
    effectiveThinkingOptionId,
    draftFeatures,
    draftFeatureValues,
    draftPreset,
    exitDraftPreset,
    supportsModelPresetSelection,
    presetPermissionDenied,
    applyDraftAgentProfile,
    applyDraftModelPreset,
    formState,
    setDraftFeatureValue,
    workingDir,
  ]);

  return {
    textSource,
    editText,
    replaceText,
    textReplacement,
    attachments,
    setAttachments,
    clear,
    isHydrated,
    attachmentFocusRequestId,
    composerState,
  };
}
