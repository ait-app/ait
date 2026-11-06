import { useMemo, useState, type ReactNode } from "react";
import { Bot, ChevronDown, ChevronRight } from "lucide-react-native";
import { View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import type { AgentFeature } from "@ait/protocol/agent-types";
import { Button } from "@/components/ui/button";

/** Presentation only: callers keep their existing native feature setters and atomic presets. */
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
          <Button
            variant="ghost"
            size="sm"
            leftIcon={Bot}
            trailing={
              expanded ? (
                <ChevronDown size={14} color={styles.icon.color} />
              ) : (
                <ChevronRight size={14} color={styles.icon.color} />
              )
            }
            disabled={disabled}
            accessibilityState={{ expanded }}
            testID="model-role-settings"
            style={styles.disclosure}
            onPress={() => {
              if (!expanded) onCatalogAccess?.();
              setExpanded(!expanded);
            }}
          >
            {t("providerControl.modelRoles")}
          </Button>
          {expanded ? roles.map((feature, index) => renderFeature(feature, index)) : null}
        </>
      ) : null}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  container: { gap: 4 },
  disclosure: { alignSelf: "flex-start" },
  icon: { color: theme.colors.foregroundMuted },
}));
