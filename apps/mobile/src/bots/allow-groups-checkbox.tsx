import React from "react";
import { Pressable, Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { Check } from "lucide-react-native";
import { withUnistyles } from "react-native-unistyles";
import type { Theme } from "@/styles/theme";
import { botStyles as styles } from "./styles";

const ThemedCheck = withUnistyles(Check);
const checkedIconMapping = (theme: Theme) => ({ color: theme.colors.accentForeground });

export function AllowGroupsCheckbox({
  checked,
  disabled,
  onToggle,
}: {
  checked: boolean;
  disabled: boolean;
  onToggle: () => void;
}) {
  const { t } = useTranslation();
  return (
    <Pressable
      style={[styles.row, disabled ? styles.disabled : null]}
      onPress={onToggle}
      disabled={disabled}
      accessibilityRole="checkbox"
      accessibilityLabel={t("bots.allowGroups")}
      accessibilityState={{ checked, disabled }}
      aria-checked={checked}
      testID="bot-channel-allow-groups"
    >
      <View style={[styles.checkbox, checked ? styles.checkboxChecked : null]}>
        {checked ? <ThemedCheck size={14} uniProps={checkedIconMapping} /> : null}
      </View>
      <Text style={styles.text}>{t("bots.allowGroups")}</Text>
    </Pressable>
  );
}
