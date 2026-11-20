import { useCallback, useMemo } from "react";
import { Pressable, Text, View, type PressableStateCallbackType } from "react-native";
import { useRouter, type Href } from "expo-router";
import { useTranslation } from "react-i18next";
import { Pencil } from "lucide-react-native";
import { StyleSheet, withUnistyles } from "react-native-unistyles";
import type { TerminalProfile } from "@ait/protocol/messages";
import { getTerminalProfileIcon, resolveTerminalProfiles } from "@ait/protocol/terminal-profiles";
import { TerminalProfileIcon } from "@/components/terminal-profile-icon";
import { Button } from "@/components/ui/button";
import { mutedIconColorMapping } from "@/components/ui/icon-color";
import { useDaemonConfig } from "@/hooks/use-daemon-config";
import { buildSettingsHostSectionRoute } from "@/utils/host-routes";

const ThemedPencil = withUnistyles(Pencil);

// The pencil draws at 14pt; the slop brings it up to a thumb-sized target without growing the row.
const EDIT_HIT_SLOP = { top: 12, bottom: 12, left: 12, right: 12 } as const;

function TerminalProfileChip({
  profile,
  disabled,
  onPress,
}: {
  profile: TerminalProfile;
  disabled: boolean;
  onPress: (profile: TerminalProfile) => void;
}) {
  const handlePress = useCallback(() => onPress(profile), [onPress, profile]);
  const icon = useMemo(
    () => <TerminalProfileIcon iconKey={getTerminalProfileIcon(profile)} size={14} />,
    [profile],
  );
  return (
    <Button
      testID={`workspace-tab-switcher-terminal-profile-${profile.id}`}
      variant="outline"
      size="sm"
      leftIcon={icon}
      disabled={disabled}
      onPress={handlePress}
    >
      {profile.name}
    </Button>
  );
}

/**
 * The host's terminal profiles, in the tab switcher's footer above New agent and New terminal:
 * everything that opens a tab on a compact layout is in the one sheet. The chips wrap rather than
 * scroll sideways, so every profile is visible and nothing competes with the sheet's own drag.
 */
export function TabSwitcherTerminalProfiles({
  serverId,
  disabled,
  onCreateTerminalWithProfile,
  onDismiss,
}: {
  serverId: string;
  disabled: boolean;
  onCreateTerminalWithProfile: (profile: TerminalProfile) => void;
  /** Closes the switcher before the terminal opens or the settings screen takes over. */
  onDismiss: () => void;
}) {
  const { t } = useTranslation();
  const router = useRouter();
  const { config } = useDaemonConfig(serverId);
  const profiles = useMemo(
    () => resolveTerminalProfiles(config?.terminalProfiles),
    [config?.terminalProfiles],
  );

  const handleSelect = useCallback(
    (profile: TerminalProfile) => {
      onDismiss();
      onCreateTerminalWithProfile(profile);
    },
    [onCreateTerminalWithProfile, onDismiss],
  );
  const handleEdit = useCallback(() => {
    onDismiss();
    router.push(buildSettingsHostSectionRoute(serverId, "terminals") as Href);
  }, [onDismiss, router, serverId]);

  if (profiles.length === 0) {
    return null;
  }

  return (
    <View style={styles.section} testID="workspace-tab-switcher-terminal-profiles">
      <View style={styles.header}>
        <Text style={styles.label}>{t("workspace.tabs.actions.terminalProfilesMenu")}</Text>
        <Pressable
          testID="workspace-tab-switcher-edit-terminal-profiles"
          accessibilityRole="button"
          accessibilityLabel={t("workspace.tabs.actions.editTerminalProfiles")}
          hitSlop={EDIT_HIT_SLOP}
          style={editButtonStyle}
          onPress={handleEdit}
        >
          <ThemedPencil size={14} uniProps={mutedIconColorMapping} />
        </Pressable>
      </View>
      <View style={styles.chips}>
        {profiles.map((profile) => (
          <TerminalProfileChip
            key={profile.id}
            profile={profile}
            disabled={disabled}
            onPress={handleSelect}
          />
        ))}
      </View>
    </View>
  );
}

function editButtonStyle({ pressed }: PressableStateCallbackType) {
  return [styles.editButton, pressed && styles.editButtonPressed];
}

const styles = StyleSheet.create((theme) => ({
  section: {
    gap: theme.spacing[2],
  },
  header: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
  },
  label: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
  },
  editButton: {
    padding: theme.spacing[1],
    borderRadius: theme.borderRadius.md,
  },
  editButtonPressed: {
    backgroundColor: theme.colors.surface2,
  },
  chips: {
    flexDirection: "row",
    flexWrap: "wrap",
    gap: theme.spacing[2],
  },
}));
