import { useCallback, useEffect, useMemo, useReducer, useRef } from "react";
import type { AgentProviderDefinition } from "@ait/protocol/provider-manifest";
import type {
  AgentMode,
  AgentModelDefinition,
  AgentProvider,
  ProviderSnapshotEntry,
} from "@ait/protocol/agent-types";
import { buildProviderDefinitions } from "@/utils/provider-definitions";
import {
  buildSelectableProviderSelectorProviders,
  type ProviderSelectorProvider,
} from "@/provider-selection/provider-selection";
import { filterSelectableModels } from "@/provider-selection/model-catalog";
import { OptimisticFormPreferences } from "@/create-agent-preferences/optimistic-preferences";
import { applyAgentProfilePreferences } from "@/create-agent-preferences/preferences";
import { useProvidersSnapshot } from "./use-providers-snapshot";
import {
  useFormPreferences,
  mergeProviderPreferences,
  type FormPreferences,
} from "./use-form-preferences";
import {
  resolveAgentForm,
  resolveEffectiveModel,
  normalizeSelectedModelId,
  resolveDefaultModelId,
  buildProviderDefinitionMap,
  buildProviderDefinitionMapForStatuses,
  PENDING_AGENT_FORM_RESOLUTION,
  INITIAL_USER_MODIFIED,
  RESOLVABLE_PROVIDER_STATUSES,
  SELECTABLE_PROVIDER_STATUSES,
  type FormInitialValues,
  type FormState,
  type ProviderModelsByProvider,
} from "@/provider-selection/resolve-agent-form";
import type { ModelPresetConfiguration } from "@/agent-controls/model-presets";
import type { MaterializedAgentProfile } from "@/agent-profiles";

export type { FormInitialValues } from "@/provider-selection/resolve-agent-form";

interface UseAgentFormStateOptions {
  serverId: string | null;
  workingDir: string;
  initialValues?: FormInitialValues;
  isVisible?: boolean;
  isCreateFlow?: boolean;
}

export interface UseAgentFormStateResult {
  selectedServerId: string | null;
  selectedProvider: AgentProvider | null;
  selectedMode: string;
  setModeFromUser: (modeId: string) => void;
  selectedModel: string;
  setModelFromUser: (modelId: string) => void;
  selectedThinkingOptionId: string;
  setThinkingOptionFromUser: (thinkingOptionId: string) => void;
  workingDir: string;
  providerDefinitions: AgentProviderDefinition[];
  modeOptions: AgentMode[];
  availableModels: AgentModelDefinition[];
  modelSelectorProviders: ProviderSelectorProvider[];
  isAllModelsLoading: boolean;
  isProviderModelsRefreshing: boolean;
  availableThinkingOptions: NonNullable<AgentModelDefinition["thinkingOptions"]>;
  isModelLoading: boolean;
  refreshProviderModels: (provider?: AgentProvider) => void;
  refetchProviderModelsIfStale: () => void;
  setProviderAndModelFromUser: (provider: AgentProvider, modelId: string) => void;
  applyProfileFromUser: (profile: MaterializedAgentProfile) => void;
  applyModelPresetFromUser: (
    provider: AgentProvider,
    configuration: ModelPresetConfiguration,
  ) => void;
  persistFormPreferences: () => Promise<void>;
}

function resolveSelectedProviderModes(input: {
  selectedEntry: ProviderSnapshotEntry | null;
  provider: AgentProvider | null;
  providerDefinitionMap: Map<AgentProvider, AgentProviderDefinition>;
}): AgentMode[] {
  const { selectedEntry, provider, providerDefinitionMap } = input;
  if (selectedEntry?.modes) {
    return selectedEntry.modes;
  }
  if (provider) {
    return providerDefinitionMap.get(provider)?.modes ?? [];
  }
  return [];
}

function buildAllProviderModels(
  snapshotEntries: ProviderSnapshotEntry[] | undefined,
): Map<string, AgentModelDefinition[]> {
  const map = new Map<string, AgentModelDefinition[]>();
  for (const entry of snapshotEntries ?? []) {
    map.set(entry.provider, filterSelectableModels(entry.models ?? []) ?? []);
  }
  return map;
}

function buildProviderModelsByProvider(
  snapshotEntries: ProviderSnapshotEntry[] | undefined,
): ProviderModelsByProvider {
  const map: ProviderModelsByProvider = new Map();
  for (const entry of snapshotEntries ?? []) {
    map.set(
      entry.provider,
      entry.status === "ready" ? filterSelectableModels(entry.models ?? null) : null,
    );
  }
  return map;
}

async function persistProviderPreferences(input: {
  provider: AgentProvider;
  formState: FormState;
  availableModels: AgentModelDefinition[] | null;
  updatePreferences: (
    updates: Partial<FormPreferences> | ((current: FormPreferences) => FormPreferences),
  ) => Promise<FormPreferences>;
}): Promise<void> {
  const { provider, formState, availableModels, updatePreferences } = input;
  const resolvedModel = resolveEffectiveModel(availableModels, formState.model);
  const modelId = resolvedModel?.id ?? formState.model;
  await updatePreferences((current) =>
    mergeProviderPreferences({
      preferences: current,
      provider,
      updates: {
        model: modelId || undefined,
        mode: formState.modeId || undefined,
        ...(modelId && formState.thinkingOptionId
          ? { thinkingByModel: { [modelId]: formState.thinkingOptionId } }
          : {}),
      },
    }),
  );
}

export function useAgentFormState(options: UseAgentFormStateOptions): UseAgentFormStateResult {
  const { serverId, initialValues, workingDir, isVisible = true, isCreateFlow = true } = options;

  const { preferences, isLoading: isPreferencesLoading, updatePreferences } = useFormPreferences();
  const preferenceOverlayRef = useRef(new OptimisticFormPreferences(preferences));

  useEffect(() => {
    preferenceOverlayRef.current.reconcile(preferences);
  }, [preferences]);

  const updateCurrentPreferences = useCallback(
    async (
      updates: Partial<FormPreferences> | ((current: FormPreferences) => FormPreferences),
    ): Promise<FormPreferences> => {
      const pendingId = preferenceOverlayRef.current.begin(updates);
      try {
        const persisted = await updatePreferences(updates);
        preferenceOverlayRef.current.commit(pendingId, persisted);
        return persisted;
      } catch (error) {
        preferenceOverlayRef.current.reject(pendingId);
        throw error;
      }
    },
    [updatePreferences],
  );

  const [{ form: formState, userModified, resolution }, dispatch] = useReducer(resolveAgentForm, {
    form: { provider: null, modeId: "", model: "", thinkingOptionId: "" },
    userModified: INITIAL_USER_MODIFIED,
    resolution: PENDING_AGENT_FORM_RESOLUTION,
  });

  const {
    entries: snapshotEntries,
    isLoading: snapshotIsLoading,
    isRefreshing: snapshotIsRefreshing,
    refresh: refreshSnapshot,
    refetchIfStale: refetchSnapshotIfStale,
  } = useProvidersSnapshot(serverId, { cwd: workingDir });

  const providerDefinitions = useMemo(
    () => buildProviderDefinitions(snapshotEntries),
    [snapshotEntries],
  );
  const providerDefinitionMap = useMemo(
    () => buildProviderDefinitionMap(providerDefinitions),
    [providerDefinitions],
  );
  const resolvableProviderDefinitionMap = useMemo(
    () =>
      buildProviderDefinitionMapForStatuses({
        snapshotEntries,
        providerDefinitions,
        statuses: RESOLVABLE_PROVIDER_STATUSES,
      }),
    [snapshotEntries, providerDefinitions],
  );
  const selectableProviderDefinitionMap = useMemo(() => {
    return buildProviderDefinitionMapForStatuses({
      snapshotEntries,
      providerDefinitions,
      statuses: SELECTABLE_PROVIDER_STATUSES,
    });
  }, [snapshotEntries, providerDefinitions]);
  const allProviderModels = useMemo(
    () => buildAllProviderModels(snapshotEntries),
    [snapshotEntries],
  );
  const providerModelsByProvider = useMemo(
    () => buildProviderModelsByProvider(snapshotEntries),
    [snapshotEntries],
  );
  const modelSelectorProviders = useMemo(
    () => buildSelectableProviderSelectorProviders(snapshotEntries),
    [snapshotEntries],
  );
  const selectedEntry = useMemo(
    () =>
      formState.provider
        ? ((snapshotEntries ?? []).find((entry) => entry.provider === formState.provider) ?? null)
        : null,
    [formState.provider, snapshotEntries],
  );
  const availableModels = filterSelectableModels(selectedEntry?.models ?? null);
  const selectedProviderIsLoading = selectedEntry?.status === "loading";
  const modeOptions = resolveSelectedProviderModes({
    selectedEntry,
    provider: formState.provider,
    providerDefinitionMap,
  });
  const isModelLoading =
    resolution.status === "pending" || snapshotIsLoading || selectedProviderIsLoading;

  useEffect(() => {
    dispatch({
      type: "INPUTS_CHANGED",
      serverId,
      isVisible,
      isCreateFlow,
      isPreferencesLoading,
      hasSnapshot: snapshotEntries !== undefined,
      initialValues,
      preferences,
      providerModelsByProvider,
      allowedProviderMap: resolvableProviderDefinitionMap,
    });
  }, [
    serverId,
    isVisible,
    isCreateFlow,
    isPreferencesLoading,
    snapshotEntries,
    initialValues,
    preferences,
    providerModelsByProvider,
    resolvableProviderDefinitionMap,
  ]);

  const setProviderAndModelFromUser = useCallback(
    (provider: AgentProvider, modelId: string) => {
      if (!selectableProviderDefinitionMap.has(provider)) {
        return;
      }
      const providerDef = selectableProviderDefinitionMap.get(provider);
      const providerModels = allProviderModels.get(provider) ?? null;
      const providerPrefs = preferenceOverlayRef.current.current().providerPreferences?.[provider];
      const normalizedModelId = normalizeSelectedModelId(modelId);
      const nextModelId = normalizedModelId || resolveDefaultModelId(providerModels);

      dispatch({
        type: "SET_PROVIDER_AND_MODEL_FROM_USER",
        provider,
        modelId,
        providerDef,
        providerModels,
        providerPrefs,
      });
      void updateCurrentPreferences((current) =>
        mergeProviderPreferences({
          preferences: current,
          provider,
          updates: {
            model: nextModelId || undefined,
          },
        }),
      );
    },
    [allProviderModels, selectableProviderDefinitionMap, updateCurrentPreferences],
  );

  const applyModelPresetFromUser = useCallback(
    (provider: AgentProvider, configuration: ModelPresetConfiguration) => {
      if (!selectableProviderDefinitionMap.has(provider)) return;
      const action = {
        type: "APPLY_MODEL_PRESET_FROM_USER" as const,
        provider,
        modelId: configuration.modelId,
        thinkingOptionId: configuration.thinkingOptionId,
        providerDef: selectableProviderDefinitionMap.get(provider),
        providerModels: allProviderModels.get(provider) ?? null,
      };
      const next = resolveAgentForm({ form: formState, userModified, resolution }, action);
      dispatch(action);
      void updateCurrentPreferences((current) =>
        mergeProviderPreferences({
          preferences: current,
          provider,
          updates: {
            model: next.form.model,
            thinkingByModel: { [next.form.model]: next.form.thinkingOptionId },
            featureValues: configuration.featureValues,
          },
        }),
      ).catch((error) => console.warn("[useAgentFormState] persist model preset failed", error));
    },
    [
      allProviderModels,
      formState,
      resolution,
      selectableProviderDefinitionMap,
      updateCurrentPreferences,
      userModified,
    ],
  );

  const applyProfileFromUser = useCallback(
    (profile: MaterializedAgentProfile) => {
      const provider = profile.provider as AgentProvider;
      if (!selectableProviderDefinitionMap.has(provider)) {
        return;
      }

      const previousProvider = formState.provider;
      const providerDef = selectableProviderDefinitionMap.get(provider);
      const providerModels = allProviderModels.get(provider) ?? null;
      const providerPrefs = preferenceOverlayRef.current.current().providerPreferences?.[provider];
      const action = {
        type: "APPLY_PROFILE_FROM_USER" as const,
        provider,
        modelId: profile.modelId,
        modeId: profile.modeId,
        thinkingOptionId: profile.thinkingOptionId,
        providerDef,
        providerModels,
        providerPrefs,
      };
      const nextState = resolveAgentForm({ form: formState, userModified, resolution }, action);
      const previousProviderModeIds = previousProvider
        ? (providerDefinitionMap.get(previousProvider)?.modes.map((mode) => mode.id) ?? [])
        : [];

      dispatch(action);
      void updateCurrentPreferences((current) => {
        const { model, modeId, thinkingOptionId } = nextState.form;
        return applyAgentProfilePreferences({
          preferences: current,
          previousProvider,
          previousProviderModeIds,
          provider,
          modelId: model,
          modeId,
          thinkingOptionId,
          featureValues: profile.featureValues,
        });
      }).catch((error) => {
        console.warn("[useAgentFormState] persist profile preference failed", error);
      });
    },
    [
      allProviderModels,
      formState,
      providerDefinitionMap,
      resolution,
      selectableProviderDefinitionMap,
      updateCurrentPreferences,
      userModified,
    ],
  );

  const setModeFromUser = useCallback(
    (modeId: string) => {
      dispatch({ type: "SET_MODE_FROM_USER", modeId });
      const provider = formState.provider;
      if (provider) {
        void updateCurrentPreferences((current) =>
          mergeProviderPreferences({
            preferences: current,
            provider,
            updates: {
              mode: modeId || undefined,
            },
          }),
        );
      }
    },
    [formState.provider, updateCurrentPreferences],
  );

  const setModelFromUser = useCallback(
    (modelId: string) => {
      const provider = formState.provider;
      const providerPrefs = provider
        ? preferenceOverlayRef.current.current().providerPreferences?.[provider]
        : undefined;
      dispatch({
        type: "SET_MODEL_FROM_USER",
        modelId,
        availableModels,
        providerPrefs,
      });
      if (provider) {
        const normalizedModelId = normalizeSelectedModelId(modelId);
        const nextModelId = normalizedModelId || resolveDefaultModelId(availableModels);
        void updateCurrentPreferences((current) =>
          mergeProviderPreferences({
            preferences: current,
            provider,
            updates: {
              model: nextModelId || undefined,
            },
          }),
        );
      }
    },
    [availableModels, formState.provider, updateCurrentPreferences],
  );

  const setThinkingOptionFromUser = useCallback(
    (thinkingOptionId: string) => {
      dispatch({ type: "SET_THINKING_OPTION_FROM_USER", thinkingOptionId });
      const { provider, model: modelId } = formState;
      if (provider && modelId) {
        void updateCurrentPreferences((current) =>
          mergeProviderPreferences({
            preferences: current,
            provider,
            updates: {
              thinkingByModel: {
                [modelId]: thinkingOptionId,
              },
            },
          }),
        );
      }
    },
    [formState, updateCurrentPreferences],
  );

  const refreshProviderModels = useCallback(
    (provider?: AgentProvider) => {
      void refreshSnapshot(provider ? [provider] : undefined);
    },
    [refreshSnapshot],
  );

  const refetchProviderModelsIfStale = useCallback(() => {
    refetchSnapshotIfStale(formState.provider);
  }, [formState.provider, refetchSnapshotIfStale]);

  const persistFormPreferences = useCallback(async () => {
    if (!formState.provider) {
      return;
    }
    await persistProviderPreferences({
      provider: formState.provider,
      formState,
      availableModels,
      updatePreferences: updateCurrentPreferences,
    });
  }, [availableModels, formState, updateCurrentPreferences]);

  const effectiveModel = resolveEffectiveModel(availableModels, formState.model);
  const availableThinkingOptionsRaw = effectiveModel?.thinkingOptions;
  const availableThinkingOptions = useMemo(
    () => availableThinkingOptionsRaw ?? [],
    [availableThinkingOptionsRaw],
  );
  return useMemo(
    () => ({
      selectedServerId: serverId,
      selectedProvider: formState.provider,
      selectedMode: formState.modeId,
      setModeFromUser,
      selectedModel: formState.model,
      setModelFromUser,
      selectedThinkingOptionId: formState.thinkingOptionId,
      setThinkingOptionFromUser,
      workingDir,
      providerDefinitions,
      modeOptions,
      availableModels: availableModels ?? [],
      modelSelectorProviders,
      isAllModelsLoading: isModelLoading,
      isProviderModelsRefreshing: snapshotIsRefreshing,
      availableThinkingOptions,
      isModelLoading,
      refreshProviderModels,
      refetchProviderModelsIfStale,
      setProviderAndModelFromUser,
      applyProfileFromUser,
      applyModelPresetFromUser,
      persistFormPreferences,
    }),
    [
      serverId,
      formState.provider,
      formState.modeId,
      formState.model,
      formState.thinkingOptionId,
      workingDir,
      setModeFromUser,
      setModelFromUser,
      setThinkingOptionFromUser,
      providerDefinitions,
      modeOptions,
      availableModels,
      modelSelectorProviders,
      snapshotIsRefreshing,
      availableThinkingOptions,
      isModelLoading,
      refreshProviderModels,
      refetchProviderModelsIfStale,
      setProviderAndModelFromUser,
      applyProfileFromUser,
      applyModelPresetFromUser,
      persistFormPreferences,
    ],
  );
}

export type CreateAgentInitialValues = FormInitialValues;
