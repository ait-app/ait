import type { ComponentType } from "react";
import { ChevronDown, ChevronRight } from "lucide-react-native";
import { Pressable, Text, type PressableStateCallbackType } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import { settingsStyles } from "@/styles/settings";

export function SettingsNavigationRow({
  label,
  value,
  icon: Icon,
  onPress,
  disabled = false,
  expanded,
  testID,
}: {
  label: string;
  value?: string;
  icon: ComponentType<{ size: number; color: string }>;
  onPress: () => void;
  disabled?: boolean;
  expanded?: boolean;
  testID?: string;
}) {
  const Chevron = expanded ? ChevronDown : ChevronRight;
  return (
    <Pressable
      onPress={onPress}
      disabled={disabled}
      accessibilityRole="button"
      accessibilityLabel={label}
      accessibilityState={{ disabled, ...(expanded === undefined ? {} : { expanded }) }}
      aria-expanded={expanded}
      testID={testID}
      style={({ pressed, hovered }: PressableStateCallbackType & { hovered?: boolean }) => [
        styles.row,
        (pressed || hovered) && styles.active,
        disabled && styles.disabled,
      ]}
    >
      <Icon size={18} color={styles.icon.color} />
      <Text style={[settingsStyles.rowTitle, styles.label]}>{label}</Text>
      {value ? (
        <Text numberOfLines={1} style={styles.value}>
          {value}
        </Text>
      ) : null}
      <Chevron size={16} color={styles.icon.color} />
    </Pressable>
  );
}

const styles = StyleSheet.create((theme) => ({
  row: {
    flexDirection: "row",
    alignItems: "center",
    minHeight: 48,
    gap: theme.spacing[3],
    paddingHorizontal: theme.spacing[4],
    paddingVertical: theme.spacing[2],
    borderRadius: theme.borderRadius.lg,
  },
  label: { flex: 1 },
  value: { maxWidth: "45%", color: theme.colors.foregroundMuted, fontSize: theme.fontSize.base },
  active: { backgroundColor: theme.colors.surface2 },
  disabled: { opacity: 0.5 },
  icon: { color: theme.colors.foregroundMuted },
}));
