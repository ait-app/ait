import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { ExternalLink } from "lucide-react-native";
import { withUnistyles } from "react-native-unistyles";
import { ContextMenuItem } from "@/components/ui/context-menu";
import { DropdownMenuItem } from "@/components/ui/dropdown-menu";
import { getIsElectron } from "@/constants/platform";
import { useToast } from "@/contexts/toast-context";
import { getDesktopHost } from "@/desktop/host";
import type { Theme } from "@/styles/theme";

const ThemedExternalLink = withUnistyles(ExternalLink);
const foregroundMutedColorMapping = (theme: Theme) => ({
  color: theme.colors.foregroundMuted,
});
const leadingIcon = <ThemedExternalLink size={14} uniProps={foregroundMutedColorMapping} />;

export function OpenWorkspaceInNewWindowMenuItem({
  serverId,
  workspaceId,
  testID,
  surface,
}: {
  serverId?: string;
  workspaceId?: string;
  testID: string;
  surface: "context" | "dropdown";
}) {
  const { t } = useTranslation();
  const toast = useToast();
  const openNew = getDesktopHost()?.window?.openNew;
  const canOpen = getIsElectron() && Boolean(serverId?.trim() && workspaceId?.trim() && openNew);
  const openInNewWindow = useCallback(async () => {
    if (!canOpen || !serverId || !workspaceId || !openNew) return;
    try {
      await openNew({ workspace: { serverId, workspaceId } });
    } catch (error) {
      console.warn("[workspace] openNew failed", error);
      toast.error(t("sidebar.project.actions.openNewWindowFailed"));
    }
  }, [canOpen, openNew, serverId, workspaceId, t, toast]);

  if (!canOpen) return null;
  const MenuItem = surface === "context" ? ContextMenuItem : DropdownMenuItem;
  return (
    <MenuItem testID={testID} leading={leadingIcon} onSelect={openInNewWindow}>
      {t("sidebar.project.actions.openNewWindow")}
    </MenuItem>
  );
}
