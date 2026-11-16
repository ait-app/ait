import { DropdownMenu, DropdownMenuContent, DropdownMenuItem } from "@/components/ui/dropdown-menu";
import { DropdownTrigger } from "@/components/ui/dropdown-trigger";
import { Switch } from "@/components/ui/switch";
import { useIsCompactFormFactor } from "@/constants/layout";
import { settingsStyles } from "@/styles/settings";
import { Children, isValidElement, useCallback, useMemo, type ReactNode } from "react";
import { Text, View } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import type { SettingsRowProps, SettingsSelectProps, SettingsSwitchProps } from "./types";
export { SettingsGroup } from "./headings/settings-group";
export { SettingsSection } from "./headings/settings-section";

interface AppSettingsRowProps extends Omit<SettingsRowProps, "hint"> {
  hint?: ReactNode;
}

export function SettingsCard({ children, testID }: { children: ReactNode; testID?: string }) {
  return (
    <View style={settingsStyles.card} testID={testID}>
      {Children.toArray(children).map((child, index) => (
        <View
          key={isValidElement(child) ? child.key : index}
          style={index ? settingsStyles.rowBorder : undefined}
        >
          {child}
        </View>
      ))}
    </View>
  );
}

export function SettingsRow({ label, hint, error, children, testID }: AppSettingsRowProps) {
  const compact = useIsCompactFormFactor();
  const rowStyle = useMemo(() => [settingsStyles.row, compact && styles.compactRow], [compact]);
  return (
    <View style={rowStyle} testID={testID}>
      <View style={styles.label}>
        <Text style={settingsStyles.rowTitle}>{label}</Text>
        {typeof hint === "string" ? <Text style={settingsStyles.rowHint}>{hint}</Text> : hint}
        {error ? (
          <Text accessibilityRole="alert" style={settingsStyles.rowError}>
            {error}
          </Text>
        ) : null}
      </View>
      {children ? <View style={styles.control}>{children}</View> : null}
    </View>
  );
}

export function SettingsSwitch({ value, onValueChange, ...row }: SettingsSwitchProps) {
  return (
    <SettingsRow {...row}>
      <Switch value={value} onValueChange={onValueChange} accessibilityLabel={row.label} />
    </SettingsRow>
  );
}

/** A dropdown item that reports its value when picked. */
export function ValueMenuItem<Value extends string>({
  value,
  label,
  selected,
  onChange,
}: {
  value: Value;
  label: string;
  selected: boolean;
  onChange: (value: Value) => void;
}) {
  const handleSelect = useCallback(() => onChange(value), [onChange, value]);
  return (
    <DropdownMenuItem selected={selected} onSelect={handleSelect}>
      {label}
    </DropdownMenuItem>
  );
}

export function SettingsSelect<Value extends string>({
  value,
  options,
  onValueChange,
  ...row
}: SettingsSelectProps<Value>) {
  return (
    <SettingsRow {...row}>
      <DropdownMenu>
        <DropdownTrigger accessibilityRole="button" accessibilityLabel={row.label}>
          <Text style={styles.value}>
            {options.find((option) => option.value === value)?.label ?? value}
          </Text>
        </DropdownTrigger>
        <DropdownMenuContent side="bottom" align="end" width={220}>
          {options.map((option) => (
            <ValueMenuItem
              key={option.value}
              value={option.value}
              label={option.label}
              selected={option.value === value}
              onChange={onValueChange}
            />
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </SettingsRow>
  );
}

const styles = StyleSheet.create((theme) => ({
  compactRow: { flexWrap: "wrap", gap: theme.spacing[3] },
  label: { flexGrow: 1, flexShrink: 1, flexBasis: 160, marginRight: theme.spacing[3] },
  control: { flexShrink: 1, maxWidth: "100%" },
  value: { color: theme.colors.foreground, fontSize: theme.fontSize.base },
}));
