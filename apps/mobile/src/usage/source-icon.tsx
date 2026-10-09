import { Gauge } from "lucide-react-native";
import { withUnistyles } from "react-native-unistyles";
import { getProviderIcon } from "@/components/provider-icons";
import { SvgXml } from "react-native-svg";
import type { Theme } from "@/styles/theme";

function UsageSourceIconBase({
  svg,
  size,
  color = "",
  sourceId,
}: {
  svg: string | null;
  size: number;
  color?: string;
  sourceId?: string;
}) {
  if (svg) return <SvgXml xml={svg} width={size} height={size} color={color} />;
  const Icon = sourceId ? getProviderIcon(sourceId) : Gauge;
  return <Icon size={size} color={color} />;
}

const ThemedUsageSourceIcon = withUnistyles(UsageSourceIconBase);

const mutedIconColor = (theme: Theme) => ({ color: theme.colors.foregroundMuted });

/** The source's catalog SVG, or a generic gauge when the source ships none. */
export function UsageSourceIcon({
  svg,
  size,
  sourceId,
}: {
  svg: string | null;
  size: number;
  sourceId?: string;
}) {
  return (
    <ThemedUsageSourceIcon svg={svg} size={size} sourceId={sourceId} uniProps={mutedIconColor} />
  );
}
