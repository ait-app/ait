import type { ReactElement } from "react";
import { Text, View } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import { useTranslation } from "react-i18next";
import type { AgentFeature, AgentModelDefinition } from "@ait/protocol/agent-types";
import type { ProviderPresetEntry } from "@ait/protocol/messages";
import { useProviderPresetFeatures, useProviderPresets } from "@/data/provider-presets";
import { presetModelLabel } from "./provider-preset-form";
import {
  modelPresetConfiguration,
  isModelPresetAvailable,
  supportsModelRoles,
  type ModelPresetControls,
} from "@/agent-controls/model-presets";

interface ProviderModelPresetChoices {
  presets: ProviderPresetEntry[];
  features: AgentFeature[];
}

/** Both headings and rows use the same supported store and role catalog. */
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
    presets:
      !store.error && !features.error && supportsModelRoles(features.data)
        ? (store.snapshot?.presets ?? [])
        : [],
    features: features.data ?? [],
  };
}

/** Provider presets share the settings cache and appear after the provider's model rows. */
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
  const presets =
    choices.presets.filter((preset) =>
      `${preset.name} ${preset.model}`.toLocaleLowerCase().includes(query.toLocaleLowerCase()),
    ) ?? [];
  if (presets.length === 0) return null;
  return (
    <View testID={`provider-model-presets-${provider}`}>
      <Text style={styles.heading}>{t("modelSelector.modelPresets")}</Text>
      {presets.map((preset) => {
        const disabled = !isModelPresetAvailable(preset, models);
        const label = presetModelLabel(preset.model, models);
        return (
          <View key={preset.name}>
            {renderRow({
              name: preset.name,
              modelLabel: disabled ? `${label} · ${t("providerSelection.unavailable")}` : label,
              testID: `provider-model-preset-${provider}-${preset.name}`,
              disabled,
              onPress: () => {
                if (!disabled)
                  controls.apply(provider, modelPresetConfiguration(preset, choices.features));
              },
            })}
          </View>
        );
      })}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  heading: {
    paddingHorizontal: theme.spacing[3],
    paddingTop: theme.spacing[2],
    paddingBottom: theme.spacing[1],
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
  },
}));
