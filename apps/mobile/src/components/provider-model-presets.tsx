import { useMemo, type ReactElement } from "react";
import { Text, View } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import { useTranslation } from "react-i18next";
import { useAgentPresetContext } from "@/agent-controls/preset-context";
import type { AgentFeature, AgentModelDefinition } from "@ait/protocol/agent-types";
import type { ProviderPresetEntry } from "@ait/protocol/messages";
import { formatThinkingOptionLabel } from "@/agent-controls/labels";
import { isWeb } from "@/constants/platform";
import { useProviderPresetFeatures, useProviderPresets } from "@/data/provider-presets";
import { buildModelCatalogIndex, presetModelLabel } from "./provider-preset-form";
import {
  modelPresetConfiguration,
  isModelPresetAvailable,
  type ModelPresetControls,
} from "@/agent-controls/model-presets";

export interface ProviderModelPresetChoices {
  presets: ProviderPresetEntry[];
  features: AgentFeature[];
  contextUnavailable?: boolean;
  selectedAvailable?: boolean;
}

/** Both headings and rows read the shared preset store and role catalog. */
export function useProviderModelPresetChoices({
  serverId,
  provider,
  cwd,
  enabled,
  contextAgentId,
  supportsSelection,
  contextRevision,
}: {
  serverId: string | null;
  provider: string;
  cwd?: string | null;
  enabled: boolean;
  contextAgentId?: string;
  supportsSelection?: boolean;
  contextRevision?: number;
}): ProviderModelPresetChoices {
  const useContext = Boolean(contextAgentId && supportsSelection);
  const store = useProviderPresets({ serverId, provider, cwd, enabled: enabled && !useContext });
  const context = useAgentPresetContext({
    serverId,
    agentId: contextAgentId,
    revision: contextRevision,
    enabled: enabled && useContext,
    poll: true,
  });
  const features = useProviderPresetFeatures(
    serverId ?? "",
    provider,
    cwd ?? undefined,
    enabled && (useContext || store.isSupported),
  );
  return {
    presets: useContext
      ? !context.error && !context.data?.error && context.data?.provider === provider
        ? context.data.presets
        : []
      : !store.error && !features.error
        ? (store.snapshot?.presets ?? [])
        : [],
    features: features.data ?? [],
    contextUnavailable:
      useContext &&
      Boolean(
        context.error ||
        context.data?.error ||
        (context.data && context.data.provider !== provider),
      ),
    selectedAvailable: useContext ? context.data?.selectedAvailable : undefined,
  };
}

/** Provider presets share the settings cache and appear at the top of the provider's model picker. */
export function ProviderModelPresets({
  provider,
  choices,
  controls,
  models = [],
  renderRow,
  query = "",
}: {
  provider: string;
  choices: ProviderModelPresetChoices;
  controls: ModelPresetControls;
  models?: readonly AgentModelDefinition[];
  renderRow: (row: {
    name: string;
    modelLabel: string;
    onPress: () => void;
    testID: string;
    disabled: boolean;
    selected: boolean;
  }) => ReactElement;
  query?: string;
}) {
  const { t } = useTranslation();
  const catalogIndex = useMemo(() => buildModelCatalogIndex(models), [models]);
  const normalizedQuery = query.trim().toLocaleLowerCase();
  const presets = useMemo(() => {
    if (controls.permissionDenied) return [];
    if (!normalizedQuery) return choices.presets;
    return choices.presets.filter((preset) => {
      const label = presetModelLabel(preset.model, catalogIndex);
      return `${preset.name} ${preset.model} ${label}`
        .toLocaleLowerCase()
        .includes(normalizedQuery);
    });
  }, [catalogIndex, choices.presets, controls.permissionDenied, normalizedQuery]);

  if (
    presets.length === 0 &&
    !choices.contextUnavailable &&
    choices.selectedAvailable !== false &&
    !controls.permissionDenied
  )
    return null;

  return (
    <View testID={`provider-model-presets-${provider}`}>
      <View style={styles.headingRow}>
        <Text style={styles.heading}>{t("modelSelector.modelPresets")}</Text>
      </View>
      {controls.permissionDenied ? (
        <Text style={styles.heading}>{t("modelSelector.presetOwnerOnly")}</Text>
      ) : null}
      {choices.contextUnavailable || choices.selectedAvailable === false ? (
        <Text style={styles.heading}>{t("modelSelector.presetContextUnavailable")}</Text>
      ) : null}
      {presets.map((preset) => {
        const unsupported =
          controls.supported === false || !preset.source || controls.permissionDenied;
        const disabled = unsupported || !isModelPresetAvailable(preset, models);
        const baseLabel = presetModelLabel(preset.model, catalogIndex);
        const thinkingLabel = preset.thinkingOptionId
          ? formatThinkingOptionLabel({ id: preset.thinkingOptionId }, t)
          : null;
        const summary = thinkingLabel ? `${baseLabel} · ${thinkingLabel}` : baseLabel;
        const selected =
          choices.selectedAvailable !== false &&
          !choices.contextUnavailable &&
          controls.selected?.provider === provider &&
          controls.selected.name === preset.name &&
          controls.selected.source === preset.source;
        return (
          <View key={preset.name}>
            {renderRow({
              name: preset.name,
              modelLabel: controls.permissionDenied
                ? `${summary} · ${t("modelSelector.presetOwnerOnly")}`
                : unsupported
                  ? `${summary} · ${t("modelSelector.presetRequiresUpgrade")}`
                  : disabled
                    ? `${summary} · ${t("providerSelection.unavailable")}`
                    : selected
                      ? `${summary} · ${t("modelSelector.presetEditInSettings")}`
                      : summary,
              testID: `provider-model-preset-${provider}-${preset.name}`,
              disabled,
              selected,
              onPress: () => {
                if (!disabled) {
                  controls.apply(
                    provider,
                    modelPresetConfiguration(preset, choices.features, models),
                    preset,
                  );
                }
              },
            })}
          </View>
        );
      })}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  headingRow: {
    paddingHorizontal: isWeb ? theme.spacing[3] : theme.spacing[6],
    paddingTop: theme.spacing[2],
    paddingBottom: theme.spacing[1],
  },
  heading: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
    fontWeight: theme.fontWeight.medium,
  },
}));
