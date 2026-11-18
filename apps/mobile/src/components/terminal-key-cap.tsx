import { useCallback, useMemo, type ComponentType } from "react";
import { Pressable, Text, type PressableStateCallbackType } from "react-native";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import { isNative } from "@/constants/platform";

// "key" sends a key to the terminal; "utility" acts on the app (paste, keyboard) instead.
type TerminalKeyCapTone = "key" | "utility";

export interface TerminalKeyCapProps {
  testID: string;
  accessibilityLabel: string;
  onPress: () => void;
  label?: string;
  icon?: ComponentType<{ color: string; size: number; strokeWidth?: number }>;
  tone?: TerminalKeyCapTone;
  // Set on a modifier only: true while it waits to combine with the next key.
  latched?: boolean;
  disabled?: boolean;
}

const KEY_CAP_HEIGHT = 38;
const KEY_CAP_ICON_SIZE = 18;
const KEY_CAP_ICON_STROKE_WIDTH = 2;
// The caps sit 6px apart. Half of that on each side makes a 38px cap a 44px touch target,
// provided the parent leaves the same room around it (see the row style in the key strip).
export const TERMINAL_KEY_CAP_HIT_SLOP = 3;

// A latched modifier is a pressed toggle button. Native has no pressed state to announce,
// so it reports "selected" there instead.
function latchedAccessibilityProps(latched: boolean | undefined) {
  if (latched === undefined) return null;
  return isNative ? { "aria-selected": latched } : { "aria-pressed": latched };
}

export function TerminalKeyCap({
  testID,
  accessibilityLabel,
  onPress,
  label,
  icon: Icon,
  tone = "key",
  latched,
  disabled = false,
}: TerminalKeyCapProps) {
  const { theme } = useUnistyles();
  const pressableStyle = useCallback(
    ({ hovered, pressed }: PressableStateCallbackType & { hovered?: boolean }) => {
      const isHeld = !disabled && (Boolean(hovered) || pressed);
      return [
        styles.cap,
        tone === "utility" && styles.capUtility,
        isHeld && (tone === "utility" ? styles.capUtilityHeld : styles.capHeld),
        latched && styles.capLatched,
        latched && isHeld && styles.capLatchedHeld,
        disabled && styles.capDisabled,
      ];
    },
    [disabled, latched, tone],
  );
  const legendStyle = useMemo(() => [styles.legend, latched && styles.legendLatched], [latched]);
  const accessibilityState = useMemo(() => ({ disabled }), [disabled]);
  let iconColor = tone === "utility" ? theme.colors.foregroundMuted : theme.colors.foreground;
  if (latched) iconColor = theme.colors.primaryForeground;

  return (
    <Pressable
      accessibilityLabel={accessibilityLabel}
      accessibilityRole="button"
      accessibilityState={accessibilityState}
      {...latchedAccessibilityProps(latched)}
      disabled={disabled}
      hitSlop={TERMINAL_KEY_CAP_HIT_SLOP}
      testID={testID}
      onPress={onPress}
      style={pressableStyle}
    >
      {Icon ? (
        <Icon color={iconColor} size={KEY_CAP_ICON_SIZE} strokeWidth={KEY_CAP_ICON_STROKE_WIDTH} />
      ) : (
        <Text style={legendStyle} numberOfLines={1} adjustsFontSizeToFit minimumFontScale={0.8}>
          {label}
        </Text>
      )}
    </Pressable>
  );
}

const styles = StyleSheet.create((theme) => ({
  cap: {
    flex: 1,
    minWidth: 0,
    height: KEY_CAP_HEIGHT,
    borderRadius: theme.borderRadius.lg,
    alignItems: "center",
    justifyContent: "center",
    paddingHorizontal: theme.spacing[1],
    backgroundColor: theme.colors.surface2,
  },
  capHeld: {
    backgroundColor: theme.colors.surface3,
  },
  capUtility: {
    backgroundColor: "transparent",
  },
  capUtilityHeld: {
    backgroundColor: theme.colors.surface2,
  },
  // A latched modifier inverts, the way a phone keyboard's Shift does, so it reads as
  // "held down" in every theme without borrowing the accent that means success elsewhere.
  capLatched: {
    backgroundColor: theme.colors.primary,
  },
  capLatchedHeld: {
    opacity: 0.85,
  },
  capDisabled: {
    opacity: theme.opacity[50],
  },
  legend: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.sm,
    fontWeight: theme.fontWeight.semibold,
    letterSpacing: 0.2,
    textAlign: "center",
  },
  legendLatched: {
    color: theme.colors.primaryForeground,
  },
}));
