import { memo } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import type { ProviderUiState } from "@ait/protocol/messages";
import { composerPillStyles } from "@/composer/pill-styles";

type Vibe = NonNullable<ProviderUiState["vibe"]>;

export const ProviderVibe = memo(function ProviderVibe({ state }: { state: Vibe }) {
  const { t } = useTranslation();
  if (!state.enabled) return null;
  return (
    <View style={composerPillStyles.body} accessibilityLiveRegion="polite" testID="provider-vibe">
      <Text style={composerPillStyles.label}>
        {t("providerControl.vibe.title")} <Text style={{ fontWeight: "700" }}>ON</Text>
      </Text>
    </View>
  );
});
