import { useState } from "react";
import { Text } from "react-native";
import { ChevronsUpDown, CircleUserRound, Import, Settings } from "lucide-react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet, withUnistyles } from "react-native-unistyles";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { extraMutedIconColorMapping, mutedIconColorMapping } from "@/components/ui/icon-color";
import { Shortcut } from "@/components/ui/shortcut";
import { useShortcutKeys } from "@/hooks/use-shortcut-keys";
import { useAccountState } from "@/runtime/account-state";
import { ICON_SIZE } from "@/styles/theme";
import { type AccountExpiry, useAccountExpiry } from "./account-expiry";
import { SidebarHelpMenuItems } from "./sidebar-help-menu";

const ThemedChevronsUpDown = withUnistyles(ChevronsUpDown);
const ThemedCircleUserRound = withUnistyles(CircleUserRound);
const ThemedImport = withUnistyles(Import);
const ThemedSettings = withUnistyles(Settings);
const settingsLeadingIcon = <ThemedSettings size={ICON_SIZE.sm} uniProps={mutedIconColorMapping} />;
const importLeadingIcon = <ThemedImport size={ICON_SIZE.sm} uniProps={mutedIconColorMapping} />;

/** Whether the account button replaces the footer's own Import, Help and Settings buttons. */
export function useIsAccountSignedIn(): boolean {
  return useAccountState((state) => state.status !== "logged_out");
}

/**
 * The signed-in footer button: who is signed in and how long the account has left, on one line.
 * It opens the menu that holds what the three footer buttons it replaces used to do.
 */
export function SidebarAccountMenu({
  onOpenSettings,
  onImportSession,
}: {
  onOpenSettings: () => void;
  onImportSession: () => void;
}) {
  const { t } = useTranslation();
  const accountName = useAccountState((state) => state.name);
  const expiry = useAccountExpiry();
  const settingsKeys = useShortcutKeys("toggle-settings");
  const [open, setOpen] = useState(false);

  const name = accountName || t("sidebar.account.fallbackName");
  const expiryLabel = useExpiryLabel(expiry);

  return (
    <DropdownMenu open={open} onOpenChange={setOpen}>
      <DropdownMenuTrigger
        style={({ hovered, pressed }) => [
          styles.trigger,
          (hovered || pressed || open) && styles.triggerActive,
        ]}
        testID="sidebar-account"
        accessibilityRole="button"
        accessibilityLabel={expiryLabel ? `${name}, ${expiryLabel}` : name}
      >
        <ThemedCircleUserRound size={ICON_SIZE.md} uniProps={mutedIconColorMapping} />
        <Text style={styles.name} numberOfLines={1} testID="sidebar-account-name">
          {name}
        </Text>
        {expiryLabel ? (
          <Text style={expiryStyle(expiry)} numberOfLines={1} testID="sidebar-account-expiry">
            {expiryLabel}
          </Text>
        ) : null}
        <ThemedChevronsUpDown size={ICON_SIZE.sm} uniProps={extraMutedIconColorMapping} />
      </DropdownMenuTrigger>
      <DropdownMenuContent
        side="top"
        align="end"
        offset={8}
        width={280}
        testID="sidebar-account-menu"
      >
        <DropdownMenuItem
          testID="sidebar-account-settings"
          leading={settingsLeadingIcon}
          trailing={settingsKeys ? <Shortcut chord={settingsKeys} /> : null}
          onSelect={onOpenSettings}
        >
          {t("sidebar.actions.settings")}
        </DropdownMenuItem>
        <DropdownMenuItem
          testID="sidebar-account-import-session"
          leading={importLeadingIcon}
          onSelect={onImportSession}
        >
          {t("importSession.title")}
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <SidebarHelpMenuItems />
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function useExpiryLabel(expiry: AccountExpiry): string | null {
  const { t } = useTranslation();
  switch (expiry.kind) {
    case "unknown":
      return null;
    case "never":
      return t("sidebar.account.expiry.never");
    case "months":
      return t("sidebar.account.expiry.months");
    case "weeks":
      return t("sidebar.account.expiry.weeks");
    case "days":
      return t("sidebar.account.expiry.days", { count: expiry.count });
    case "hours":
      return t("sidebar.account.expiry.hours", { count: expiry.count });
    case "expired":
      return t("sidebar.account.expiry.expired");
  }
}

function expiryStyle(expiry: AccountExpiry) {
  if (expiry.kind === "expired") return styles.expiryExpired;
  // Less than a week left is the part that is counted.
  if (expiry.kind === "days" || expiry.kind === "hours") return styles.expirySoon;
  return styles.expiry;
}

const styles = StyleSheet.create((theme) => ({
  trigger: {
    flex: 1,
    minWidth: 0,
    minHeight: 32,
    flexDirection: "row",
    alignItems: "center",
    gap: theme.spacing[2],
    paddingVertical: theme.spacing[1],
    paddingHorizontal: theme.spacing[2],
    borderRadius: theme.borderRadius.lg,
    userSelect: "none",
  },
  triggerActive: {
    backgroundColor: theme.colors.surfaceSidebarHover,
  },
  // The name gives way first, so the time left is always readable.
  name: {
    flex: 1,
    minWidth: 0,
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
    fontWeight: theme.fontWeight.medium,
  },
  expiry: {
    flexShrink: 0,
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
  },
  expirySoon: {
    flexShrink: 0,
    color: theme.colors.statusWarning,
    fontSize: theme.fontSize.sm,
  },
  expiryExpired: {
    flexShrink: 0,
    color: theme.colors.statusDanger,
    fontSize: theme.fontSize.sm,
  },
}));
