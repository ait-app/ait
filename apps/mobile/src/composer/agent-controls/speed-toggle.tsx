import { useCallback } from "react";
import { Text } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import type { AgentFeature } from "@ait/protocol/agent-types";
import { getAgentFeatureIcon } from "@/agent-controls/icons";
import { formatAgentFeatureLabel } from "@/agent-controls/labels";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { AgentControlTrigger } from "@/composer/agent-controls/control";

export function SpeedToggle({
  feature,
  surface,
  disabled,
  onSetFeature,
  onActionComplete,
}: {
  feature: Extract<AgentFeature, { type: "select" }>;
  surface: "toolbar" | "sheet";
  disabled: boolean;
  onSetFeature?: (featureId: string, value: unknown) => void;
  onActionComplete?: () => void;
}) {
  const { theme } = useUnistyles();
  const { t } = useTranslation();
  const enabled = feature.value !== null && feature.value !== "default";
  const fastOption = feature.options.find(
    (option) => option.id === "fast" || option.id === "priority",
  );
  const label = formatAgentFeatureLabel(feature, t);
  const value = t(enabled ? "agentControls.speed.fast" : "agentControls.speed.normal");
  const tooltip = t("agentControls.speed.withValue", { value });
  const Icon = getAgentFeatureIcon(feature.icon, feature.id);
  const handlePress = useCallback(() => {
    onSetFeature?.(feature.id, enabled ? "default" : "fast");
    onActionComplete?.();
  }, [enabled, feature.id, onActionComplete, onSetFeature]);

  if (!fastOption) return null;

  const trigger = (
    <AgentControlTrigger
      icon={Icon}
      iconColor={enabled ? theme.colors.palette.yellow[400] : theme.colors.foregroundMuted}
      surface={surface}
      label={label}
      value={value}
      showToolbarLabel={false}
      disabled={disabled}
      onPress={handlePress}
      accessibilityLabel={tooltip}
      accessibilityState={{ selected: enabled, disabled }}
      testID={`agent-feature-${feature.id}`}
    />
  );

  if (surface === "sheet") return trigger;

  return (
    <Tooltip delayDuration={0} enabledOnDesktop enabledOnMobile={false}>
      <TooltipTrigger asChild triggerRefProp="ref">
        {trigger}
      </TooltipTrigger>
      <TooltipContent side="top" align="center" offset={8}>
        <Text style={styles.tooltipText}>{tooltip}</Text>
      </TooltipContent>
    </Tooltip>
  );
}

const styles = StyleSheet.create((theme) => ({
  tooltipText: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
    lineHeight: theme.fontSize.base * 1.4,
  },
}));
