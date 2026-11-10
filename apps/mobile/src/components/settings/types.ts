import type { ReactNode } from "react";

export interface SettingsSectionProps {
  title: string;
  info?: ReactNode;
  trailing?: ReactNode;
  children: ReactNode;
  testID?: string;
}
export interface SettingsRowProps {
  label: string;
  hint?: string;
  error?: string | null;
  children?: ReactNode;
  testID?: string;
}
export interface SettingsSwitchProps extends SettingsRowProps {
  value: boolean;
  onValueChange(value: boolean): void;
  disabled?: boolean;
}
export interface SettingsSelectProps<Value extends string = string> extends SettingsRowProps {
  value: Value;
  options: readonly { label: string; value: Value }[];
  onValueChange(value: Value): void;
  disabled?: boolean;
}
