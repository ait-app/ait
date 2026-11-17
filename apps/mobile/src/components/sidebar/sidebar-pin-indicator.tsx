import { Pin } from "lucide-react-native";
import { useTranslation } from "react-i18next";
import { View } from "react-native";
import { StyleSheet, withUnistyles } from "react-native-unistyles";
import { mutedIconColorMapping } from "@/components/ui/icon-color";

const ThemedPin = withUnistyles(Pin);

export function SidebarPinIndicator({ testID }: { testID: string }) {
  const { t } = useTranslation();
  return (
    <View
      style={styles.indicator}
      testID={testID}
      accessibilityLabel={t("sidebar.pinned.title")}
      pointerEvents="none"
    >
      <ThemedPin size={12} uniProps={mutedIconColorMapping} />
    </View>
  );
}

const styles = StyleSheet.create({
  indicator: {
    width: 12,
    height: 20,
    flexShrink: 0,
    alignItems: "center",
    justifyContent: "center",
  },
});
