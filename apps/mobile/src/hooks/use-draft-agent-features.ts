import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { AgentFeature, AgentProvider, AgentSessionConfig } from "@ait/protocol/agent-types";
import { useHostRuntimeClient, useHostRuntimeIsConnected } from "@/runtime/host-runtime";
import { mergeProviderPreferences, useFormPreferences } from "./use-form-preferences";
import { useProvidersSnapshot } from "./use-providers-snapshot";
import {
  isModelConfigurationFeature,
  modelPresetConfiguration,
  withModelPresetSelection,
} from "@/agent-controls/model-presets";
import type { AgentConfigApply } from "@ait/protocol/messages";
import { useToast } from "@/contexts/toast-context";
import {
  applyFeatureValues,
  pruneFeatureValues,
  resolveFeatureValues,
} from "./feature-preferences";

type DraftFeatureConfig = Pick<
  AgentSessionConfig,
  "provider" | "cwd" | "modeId" | "model" | "thinkingOptionId"
>;

export function useDraftAgentFeatures(input: {
  serverId: string | null | undefined;
  provider: AgentProvider | null;
  cwd: string | null | undefined;
  modeId: string | null | undefined;
  modelId: string | null | undefined;
  thinkingOptionId: string | null | undefined;
  initialFeatureValues?: Record<string, unknown>;
  onApplyModelPreset?: (config: AgentConfigApply) => void;
}) {
  const { t } = useTranslation();
  const toast = useToast();
  const { serverId, provider, cwd, modeId, modelId, thinkingOptionId, initialFeatureValues } =
    input;
  const [localFeatureValues, setLocalFeatureValues] = useState<Record<string, unknown>>(
    () => initialFeatureValues ?? {},
  );
  const client = useHostRuntimeClient(serverId ?? "");
  const isConnected = useHostRuntimeIsConnected(serverId ?? "");
  const { preferences, updatePreferences } = useFormPreferences();
  const normalizedCwd = cwd?.trim() || "";
  const normalizedProvider = provider ?? null;
  const previousProviderRef = useRef<AgentProvider | null>(normalizedProvider);
  const persistedFeatureValues = useMemo(
    () => (provider ? (preferences.providerPreferences?.[provider]?.featureValues ?? {}) : {}),
    [preferences.providerPreferences, provider],
  );

  const draftConfig = useMemo<DraftFeatureConfig | null>(() => {
    if (!normalizedProvider || !normalizedCwd) {
      return null;
    }

    return {
      provider: normalizedProvider,
      cwd: normalizedCwd,
      ...(modeId ? { modeId } : {}),
      ...(modelId ? { model: modelId } : {}),
      ...(thinkingOptionId ? { thinkingOptionId } : {}),
    };
  }, [modeId, modelId, normalizedCwd, normalizedProvider, thinkingOptionId]);

  const { entries } = useProvidersSnapshot(serverId ?? null, {
    cwd: normalizedCwd,
    enabled: normalizedProvider === "omp",
    refreshOnAccess: false,
  });
  const catalogFetchedAt = entries?.find((entry) => entry.provider === "omp")?.fetchedAt;
  const featuresQuery = useQuery({
    queryKey: [
      "providerFeatures",
      serverId ?? null,
      normalizedProvider,
      normalizedCwd || null,
      modeId ?? null,
      modelId ?? null,
      thinkingOptionId ?? null,
      normalizedProvider === "omp" ? catalogFetchedAt : null,
    ],
    enabled: Boolean(serverId && client && isConnected && draftConfig),
    staleTime: 5 * 60 * 1000,
    queryFn: async () => {
      if (!client || !draftConfig) {
        throw new Error(t("workspace.terminal.hostDisconnected"));
      }
      const payload = await client.listProviderFeatures(draftConfig);
      if (payload.error) {
        throw new Error(payload.error);
      }
      return payload.features ?? [];
    },
  });
  const featureScope = JSON.stringify([serverId ?? null, normalizedProvider, normalizedCwd]);
  const retainedFeatures = useRef<{ scope: string; features: AgentFeature[] } | null>(null);
  if (featuresQuery.data !== undefined) {
    retainedFeatures.current = { scope: featureScope, features: featuresQuery.data };
  }
  const availableFeaturesRaw =
    featuresQuery.data ??
    (retainedFeatures.current?.scope === featureScope
      ? retainedFeatures.current.features
      : undefined);
  const availableFeatures = useMemo(() => availableFeaturesRaw ?? [], [availableFeaturesRaw]);
  const featureValues = useMemo(
    () =>
      resolveFeatureValues({
        features: availableFeatures,
        persistedFeatureValues,
        localFeatureValues,
      }),
    [availableFeatures, localFeatureValues, persistedFeatureValues],
  );

  // Any provider that advertises the preset control gets the same label reconciliation, so a
  // manual model edit falls back to "custom" for OMP and Ait-backed providers alike.
  const features = useMemo(() => {
    const applied = applyFeatureValues(availableFeatures, featureValues);
    return applied.some(isModelConfigurationFeature)
      ? withModelPresetSelection(applied, { modelId, thinkingOptionId, featureValues })
      : applied;
  }, [availableFeatures, featureValues, modelId, thinkingOptionId]);

  useEffect(() => {
    const previousProvider = previousProviderRef.current;
    previousProviderRef.current = normalizedProvider;
    if (previousProvider === null) {
      return;
    }
    if (previousProvider !== normalizedProvider) {
      setLocalFeatureValues({});
    }
  }, [normalizedProvider]);

  useEffect(() => {
    if (availableFeaturesRaw === undefined) {
      return;
    }
    const next = pruneFeatureValues(localFeatureValues, availableFeatures);
    if (next !== localFeatureValues) {
      setLocalFeatureValues(next);
    }
  }, [availableFeatures, availableFeaturesRaw, localFeatureValues]);

  const effectiveFeatureValues = Object.keys(featureValues).length > 0 ? featureValues : undefined;
  const setFeatureValue = useCallback(
    (featureId: string, value: unknown) => {
      let preset: AgentConfigApply | null;
      try {
        preset = modelPresetConfiguration(features, featureId, value);
      } catch (error) {
        toast.error(error instanceof Error ? error.message : String(error));
        return;
      }
      if (preset) {
        setLocalFeatureValues((current) => ({ ...current, ...preset.featureValues }));
        input.onApplyModelPreset?.(preset);
        return;
      }
      setLocalFeatureValues((current) => {
        if (Object.is(current[featureId], value)) {
          return current;
        }

        return { ...current, [featureId]: value };
      });
      if (!provider) {
        return;
      }
      void updatePreferences((current) =>
        mergeProviderPreferences({
          preferences: current,
          provider,
          updates: {
            featureValues: {
              [featureId]: value,
            },
          },
        }),
      ).catch((error) => {
        console.warn("[useDraftAgentFeatures] persist feature preference failed", error);
      });
    },
    [provider, updatePreferences, features, input.onApplyModelPreset, toast],
  );

  const applyProfileFeatureValues = useCallback((values: Record<string, unknown>) => {
    setLocalFeatureValues(values);
  }, []);

  return {
    features,
    featureValues: effectiveFeatureValues,
    isLoading: featuresQuery.isLoading,
    setFeatureValue,
    applyProfileFeatureValues,
  };
}
