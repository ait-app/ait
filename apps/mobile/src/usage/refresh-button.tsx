import { RotateCw } from "lucide-react-native";
import { Button } from "@/components/ui/button";
import { useIsCompactFormFactor } from "@/constants/layout";
import { useUsageCopy } from "./copy";

/** Bypasses the host quota cache for every provider; disables repeat presses while pending. */
export function UsageRefreshButton({ busy, onRefresh }: { busy: boolean; onRefresh: () => void }) {
  const usageCopy = useUsageCopy();
  const compact = useIsCompactFormFactor();
  return (
    <Button
      variant="outline"
      size={compact ? "sm" : "xs"}
      accessibilityLabel={usageCopy.refreshAll}
      loading={busy}
      onPress={onRefresh}
      leftIcon={RotateCw}
      testID="usage-refresh-all"
    >
      {busy ? usageCopy.refreshing : usageCopy.refreshAll}
    </Button>
  );
}
