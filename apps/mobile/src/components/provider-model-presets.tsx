import { useMemo, type ReactElement } from "react";
import { Text, View } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import { useTranslation } from "react-i18next";
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
}

/** Both headings and rows read the shared preset store and role catalog. */
export function useProviderModelPresetChoices({
  serverId,
  provider,
  cwd,
  enabled,
}: {
  serverId: string | null;
  provider: string;
  cwd?: string | null;
  enabled: boolean;
}): ProviderModelPresetChoices {
  const store = useProviderPresets({ serverId, provider, cwd, enabled });
  const features = useProviderPresetFeatures(
    serverId ?? "",
    provider,
    cwd ?? undefined,
    enabled && store.isSupported,
  );
  return {
    presets: !store.error && !features.error ? (store.snapshot?.presets ?? []) : [],
    features: features.data ?? [],
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
  }) => ReactElement;
  query?: string;
}) {
  const { t } = useTranslation();
  const catalogIndex = useMemo(() => buildModelCatalogIndex(models), [models]);
  const normalizedQuery = query.trim().toLocaleLowerCase();
  const presets = useMemo(() => {
    if (!normalizedQuery) return choices.presets;
    return choices.presets.filter((preset) => {
      const label = presetModelLabel(preset.model, catalogIndex);
      return `${preset.name} ${preset.model} ${label}`
        .toLocaleLowerCase()
        .includes(normalizedQuery);
    });
  }, [catalogIndex, choices.presets, normalizedQuery]);

  if (presets.length === 0) return null;

  return (
    <View testID={`provider-model-presets-${provider}`}>
      <View style={styles.headingRow}>
        <Text style={styles.heading}>{t("modelSelector.modelPresets")}</Text>
      </View>
      {presets.map((preset) => {
        const disabled = !isModelPresetAvailable(preset, models);
        const baseLabel = presetModelLabel(preset.model, catalogIndex);
        const thinkingLabel = preset.thinkingOptionId
          ? formatThinkingOptionLabel({ id: preset.thinkingOptionId }, t)
          : null;
        const summary = thinkingLabel ? `${baseLabel} · ${thinkingLabel}` : baseLabel;
        return (
          <View key={preset.name}>
            {renderRow({
              name: preset.name,
              modelLabel: disabled ? `${summary} · ${t("providerSelection.unavailable")}` : summary,
              testID: `provider-model-preset-${provider}-${preset.name}`,
              disabled,
              onPress: () => {
                if (!disabled) {
                  controls.apply(
                    provider,
                    modelPresetConfiguration(preset, choices.features, models),
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
