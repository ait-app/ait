import { useCallback } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import { Plus } from "lucide-react-native";
import type { AgentProvider } from "@ait/protocol/agent-types";
import { Button } from "@/components/ui/button";
import { useCurrentOverlayLayer } from "@/lib/overlay-root";
import { useProviderSettingsStore } from "@/stores/provider-settings-store";
import {
  presetControlAvailable,
  useProviderPresets,
  type ProviderPresetsSnapshot,
} from "@/data/provider-presets";

/**
 * The empty preset state shown where a preset would be picked.
 *
 * Editing happens in the provider settings sheet, which is the single management surface for
 * every provider; this row only explains that and opens it above the current overlay so the
 * sheet is never hidden behind the picker that launched it.
 */
export function ModelPresetEmptyState({
  serverId,
  provider,
  cwd,
  presets: presetsProp,
}: {
  serverId: string | null | undefined;
  provider: AgentProvider | null | undefined;
  cwd: string | null | undefined;
  presets?: ProviderPresetsSnapshot | null;
}) {
  const { t } = useTranslation();
  const overlayParentLayer = useCurrentOverlayLayer();
  const openProviderSettings = useProviderSettingsStore((state) => state.open);
  const fallback = useProviderPresets({
    serverId,
    provider,
    cwd,
    enabled: presetsProp === undefined,
  });
  const presets = presetsProp ?? fallback.snapshot;

  const handlePress = useCallback(() => {
    if (!serverId || !provider) return;
    // The editor must read the store this row just read, so its directory travels with it.
    openProviderSettings({ serverId, provider, overlayParentLayer, cwd: cwd ?? null });
  }, [openProviderSettings, overlayParentLayer, provider, serverId, cwd]);

  // No store answer means the host predates the preset RPCs; show nothing rather than a
  // button that cannot lead anywhere.
  const store = presetControlAvailable(presets) ? presets : null;
  if (!store || store.presets.length > 0) {
    return null;
  }
  return (
    <View style={styles.container} testID="model-preset-empty-state">
      <Text style={styles.text}>
        {store.descriptor.backend === "native"
          ? t("providerControl.presetEmptyNative")
          : t("providerControl.presetEmptyManaged")}
      </Text>
      <Button
        variant="secondary"
        size="sm"
        leftIcon={Plus}
        onPress={handlePress}
        testID="model-preset-create"
      >
        {t("providerControl.createPreset")}
      </Button>
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  container: {
    gap: theme.spacing[2],
    paddingHorizontal: theme.spacing[2],
    paddingVertical: theme.spacing[2],
  },
  text: {
    fontSize: theme.fontSize.sm,
    color: theme.colors.foregroundMuted,
  },
}));
