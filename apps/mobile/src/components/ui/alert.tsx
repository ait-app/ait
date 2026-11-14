import { AlertTriangle, CheckCircle2, XCircle, type LucideIcon } from "lucide-react-native";
import { type ReactNode, useMemo } from "react";
import { Text, View } from "react-native";
import { StyleSheet, useUnistyles } from "react-native-unistyles";

type AlertVariant = "success" | "warning" | "error";

interface AlertProps {
  title?: string;
  description?: ReactNode;
  variant: AlertVariant;
  children?: ReactNode;
  testID?: string;
}

const VARIANT_ICON: Record<AlertVariant, LucideIcon> = {
  success: CheckCircle2,
  warning: AlertTriangle,
  error: XCircle,
};

export function Alert({ title, description, variant, children, testID }: AlertProps) {
  const { theme } = useUnistyles();
  const accentColor = resolveAccentColor(variant, theme);
  const borderColor = variant === "success" ? theme.colors.border : accentColor;

  const containerStyle = useMemo(() => [styles.container, { borderColor }], [borderColor]);

  const titleStyle = useMemo(() => [styles.title, { color: accentColor }], [accentColor]);

  const Icon = VARIANT_ICON[variant];

  const hasDescription = description != null && description !== "";

  return (
    <View style={containerStyle} testID={testID} accessibilityRole="alert">
      <View style={styles.iconSlot}>
        <Icon size={theme.iconSize.sm} color={accentColor} />
      </View>
      <View style={styles.body}>
        {title ? <Text style={titleStyle}>{title}</Text> : null}
        {hasDescription && typeof description === "string" ? (
          <Text style={styles.description}>{description}</Text>
        ) : null}
        {hasDescription && typeof description !== "string" ? (
          <View style={styles.descriptionSlot}>{description}</View>
        ) : null}
        {children ? <View style={styles.actions}>{children}</View> : null}
      </View>
    </View>
  );
}

function resolveAccentColor(
  variant: AlertVariant,
  theme: ReturnType<typeof useUnistyles>["theme"],
): string {
  if (variant === "success") return theme.colors.statusSuccess;
  if (variant === "warning") return theme.colors.palette.amber[500];
  return theme.colors.destructive;
}

const styles = StyleSheet.create((theme) => ({
  container: {
    flexDirection: "row",
    alignItems: "flex-start",
    gap: theme.spacing[3],
    borderWidth: theme.borderWidth[1],
    borderColor: theme.colors.border,
    backgroundColor: "transparent",
    borderRadius: theme.borderRadius.xl,
    paddingVertical: theme.spacing[3],
    paddingHorizontal: theme.spacing[4],
  },
  iconSlot: {
    paddingTop: 2,
  },
  body: {
    flex: 1,
    minWidth: 0,
    gap: theme.spacing[1],
  },
  title: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
    fontWeight: theme.fontWeight.medium,
  },
  description: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
  },
  descriptionSlot: {
    flexShrink: 1,
    minWidth: 0,
    gap: theme.spacing[2],
  },
  actions: {
    flexDirection: "row",
    gap: theme.spacing[2],
    marginTop: theme.spacing[2],
  },
}));
