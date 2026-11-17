import type { ReactElement } from "react";
import { SquareTerminal } from "lucide-react-native";
import { withUnistyles } from "react-native-unistyles";
import { getProviderIcon } from "@/components/provider-icons";
import { mutedIconColorMapping } from "@/components/ui/icon-color";

const ThemedSquareTerminal = withUnistyles(SquareTerminal);

function ProviderProfileIcon({
  iconKey,
  size,
  color = "",
}: {
  iconKey: string;
  size: number;
  color?: string;
}) {
  const Icon = getProviderIcon(iconKey);
  return <Icon size={size} color={color} />;
}

const ThemedProviderProfileIcon = withUnistyles(ProviderProfileIcon);

export function TerminalProfileIcon({
  iconKey,
  size = 14,
}: {
  iconKey: string | undefined;
  size?: number;
}): ReactElement {
  if (!iconKey) {
    return <ThemedSquareTerminal size={size} uniProps={mutedIconColorMapping} />;
  }
  return (
    <ThemedProviderProfileIcon iconKey={iconKey} size={size} uniProps={mutedIconColorMapping} />
  );
}
