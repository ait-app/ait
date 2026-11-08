import { Pin } from "lucide-react-native";
import { useTranslation } from "react-i18next";
import { View } from "react-native";
import { StyleSheet, withUnistyles } from "react-native-unistyles";
import type { Theme } from "@/styles/theme";

const ThemedPin = withUnistyles(Pin);
const mutedColor = (theme: Theme) => ({ color: theme.colors.foregroundMuted });

export function SidebarPinIndicator({ testID }: { testID: string }) {
  const { t } = useTranslation();
  return (
    <View
      style={styles.indicator}
      testID={testID}
      accessibilityLabel={t("sidebar.pinned.title")}
      pointerEvents="none"
    >
      <ThemedPin size={12} uniProps={mutedColor} />
    </View>
  );
}

const styles = StyleSheet.create({
  indicator: {
    height: 20,
    flexShrink: 0,
    alignItems: "center",
    justifyContent: "center",
  },
});
