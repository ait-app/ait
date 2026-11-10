import { useMemo, useState, type ReactNode } from "react";
import { Bot } from "lucide-react-native";
import { View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import type { AgentFeature } from "@ait/protocol/agent-types";
import { SettingsNavigationRow } from "@/components/settings/navigation-row";

/** Provider settings presentation; callers retain their atomic configuration setters. */
export function ModelFeatureControls({
  features,
  renderFeature,
  disabled = false,
  onCatalogAccess,
}: {
  features: readonly AgentFeature[];
  renderFeature: (feature: AgentFeature, index: number) => ReactNode;
  disabled?: boolean;
  onCatalogAccess?: () => void;
}) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const preset = features.find((feature) => feature.id === "modelPreset");
  const roles = useMemo(
    () => features.filter((feature) => feature.id.startsWith("modelRole.")),
    [features],
  );
  if (!preset && roles.length === 0) return null;
  return (
    <View style={styles.container} testID="model-feature-controls">
      {preset ? renderFeature(preset, 0) : null}
      {roles.length > 0 ? (
        <>
          <SettingsNavigationRow
            icon={Bot}
            label={t("providerControl.modelRoles")}
            disabled={disabled}
            expanded={expanded}
            testID="model-role-settings"
            onPress={() => {
              if (!expanded) onCatalogAccess?.();
              setExpanded(!expanded);
            }}
          />
          {expanded ? roles.map((feature, index) => renderFeature(feature, index)) : null}
        </>
      ) : null}
    </View>
  );
}

const styles = StyleSheet.create(() => ({
  container: { gap: 4 },
}));
