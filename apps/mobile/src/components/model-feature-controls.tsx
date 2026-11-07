import { useMemo, useState, type ReactNode } from "react";
import { Bot, ChevronDown, ChevronRight } from "lucide-react-native";
import { View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import type { AgentFeature } from "@ait/protocol/agent-types";
import type { ProviderPresetsSnapshot } from "@/data/provider-presets";
import { Button } from "@/components/ui/button";
import { ModelPresetEmptyState } from "@/components/model-preset-empty-state";

/**
 * Presentation only: callers keep their existing native feature setters and atomic presets.
 *
 * `presets` lets a provider with no preset entries yet still explain where presets are made,
 * and `settingsTarget` carries the host and provider that sheet belongs to.
 */
export function ModelFeatureControls({
  features,
  renderFeature,
  disabled = false,
  onCatalogAccess,
  presets,
  settingsTarget,
}: {
  features: readonly AgentFeature[];
  renderFeature: (feature: AgentFeature, index: number) => ReactNode;
  disabled?: boolean;
  onCatalogAccess?: () => void;
  presets?: ProviderPresetsSnapshot | null;
  settingsTarget?: {
    serverId: string | null | undefined;
    provider: string | null | undefined;
    cwd: string | null | undefined;
  };
}) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const preset = features.find((feature) => feature.id === "modelPreset");
  const roles = useMemo(
    () => features.filter((feature) => feature.id.startsWith("modelRole.")),
    [features],
  );
  // A provider advertises the preset control either by emitting one or by having a preset
  // store the editor can fill; either way the user must be able to reach the editor. The row
  // reads that store itself, so an unseeded caller still gets the entry.
  const showEmpty = !preset && settingsTarget !== undefined;
  if (!preset && roles.length === 0 && !showEmpty) return null;
  return (
    <View style={styles.container} testID="model-feature-controls">
      {preset ? renderFeature(preset, 0) : null}
      {showEmpty && settingsTarget ? (
        <ModelPresetEmptyState
          serverId={settingsTarget.serverId}
          provider={settingsTarget.provider}
          cwd={settingsTarget.cwd}
          presets={presets}
        />
      ) : null}
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
