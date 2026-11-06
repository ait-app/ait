import React from "react";
import { Pressable, Text, View } from "react-native";
import { Check } from "lucide-react-native";
import { withUnistyles } from "react-native-unistyles";
import { useTranslation } from "react-i18next";
import type { ProviderSnapshotEntry } from "@ait/protocol/agent-types";
import { BOT_TOOL_GROUPS, type BotPermissions, type BotToolGroup } from "@ait/protocol/bot/types";
import { getUnattendedModeId } from "@ait/protocol/provider-manifest";
import { useSessionStore } from "@/stores/session-store";
import type { Theme } from "@/styles/theme";
import { botStyles as styles } from "./styles";

const ThemedCheck = withUnistyles(Check);
const checkedIconMapping = (theme: Theme) => ({ color: theme.colors.accentForeground });

/** Most modes the host accepts for one provider. */
const MAX_MODES_PER_PROVIDER = 16;

/** Whether the host implements `bot.permissions.update.request`; older hosts hide the UI. */
export function useBotPermissionsSupported(serverId: string): boolean {
  return useSessionStore(
    (state) => state.sessions[serverId]?.serverInfo?.features?.workspaceBotPermissions === true,
  );
}

const GROUP_LABEL_KEYS: Record<BotToolGroup, string> = {
  observe: "bots.groupObserve",
  agents: "bots.groupAgents",
  stop: "bots.groupStop",
  schedules: "bots.groupSchedules",
  scripts: "bots.groupScripts",
  terminals: "bots.groupTerminals",
  workspaces: "bots.groupWorkspaces",
};

/** Groups whose tools run commands or create worktrees. */
const RISKY_GROUPS: ReadonlySet<BotToolGroup> = new Set(["scripts", "terminals", "workspaces"]);

function Checkbox({
  checked,
  disabled,
  label,
  onPress,
  testID,
}: {
  checked: boolean;
  disabled: boolean;
  label: string;
  onPress: () => void;
  testID: string;
}) {
  return (
    <Pressable
      style={[styles.row, disabled ? styles.disabled : null]}
      onPress={onPress}
      disabled={disabled}
      accessibilityRole="checkbox"
      accessibilityLabel={label}
      accessibilityState={{ checked, disabled }}
      aria-checked={checked}
      aria-disabled={disabled}
      testID={testID}
    >
      <View style={[styles.checkbox, checked ? styles.checkboxChecked : null]}>
        {checked ? <ThemedCheck size={14} uniProps={checkedIconMapping} /> : null}
      </View>
      <Text style={styles.text}>{label}</Text>
    </Pressable>
  );
}

/** One provider's mode choices; `modes` lists every mode the user may pick. */
interface ProviderModeChoices {
  provider: string;
  label: string;
  modes: readonly { id: string; label: string }[];
}

/**
 * Mode choices for each provider in the snapshot, followed by cap entries whose provider is no
 * longer available, so they stay visible and can be removed.
 */
function modeChoices(
  providers: ProviderSnapshotEntry[],
  modes: BotPermissions["modes"],
  unavailableLabel: (provider: string) => string,
): ProviderModeChoices[] {
  const available = new Set(providers.map((entry) => entry.provider));
  return [
    ...providers.map((entry) => ({
      provider: entry.provider,
      label: entry.label || entry.provider,
      modes: entry.modes ?? [],
    })),
    ...Object.entries(modes)
      .filter(([provider]) => !available.has(provider))
      .map(([provider, ids]) => ({
        provider,
        label: unavailableLabel(provider),
        modes: ids.map((id) => ({ id, label: id })),
      })),
  ];
}

function ProviderModes({
  choices,
  selected,
  disabled,
  onChange,
}: {
  choices: ProviderModeChoices;
  selected: string[];
  disabled: boolean;
  onChange: (modes: string[]) => void;
}) {
  const { t } = useTranslation();
  const { provider } = choices;
  const unattended = getUnattendedModeId(provider);
  const full = selected.length >= MAX_MODES_PER_PROVIDER;
  return (
    <View style={styles.permissionBlock} testID={`bot-modes-${provider}`}>
      <Text style={styles.text}>{choices.label}</Text>
      {choices.modes.map((mode) => {
        const checked = selected.includes(mode.id);
        const label = mode.id === unattended ? `⚠ ${mode.label}` : mode.label;
        return (
          <View key={mode.id} style={styles.row}>
            <Checkbox
              checked={checked}
              disabled={disabled || (!checked && full)}
              label={label}
              onPress={() =>
                onChange(checked ? selected.filter((id) => id !== mode.id) : [...selected, mode.id])
              }
              testID={`bot-mode-${provider}-${mode.id}`}
            />
            {checked && selected[0] === mode.id ? (
              <Text style={styles.hint} testID={`bot-mode-default-${provider}-${mode.id}`}>
                {t("bots.defaultMode")}
              </Text>
            ) : null}
            {checked && selected[0] !== mode.id ? (
              <Pressable
                onPress={() => onChange([mode.id, ...selected.filter((id) => id !== mode.id)])}
                disabled={disabled}
                accessibilityRole="button"
                accessibilityLabel={`${t("bots.defaultMode")}: ${mode.label}`}
                testID={`bot-mode-make-default-${provider}-${mode.id}`}
              >
                <Text style={[styles.hint, styles.link]}>{t("bots.defaultMode")}</Text>
              </Pressable>
            ) : null}
          </View>
        );
      })}
    </View>
  );
}

/** Tool-group switches and the per-provider mode cap shared by setup and settings. */
export function BotPermissionFields({
  providers,
  value,
  onChange,
  disabled,
}: {
  providers: ProviderSnapshotEntry[];
  value: BotPermissions;
  onChange: (value: BotPermissions) => void;
  disabled: boolean;
}) {
  const { t } = useTranslation();
  const toggleGroup = (group: BotToolGroup) => {
    const groups = value.groups.includes(group)
      ? value.groups.filter((id) => id !== group)
      : [...value.groups, group];
    onChange({ ...value, groups });
  };
  const setModes = (provider: string, modes: string[]) => {
    const next = { ...value.modes };
    if (modes.length === 0) delete next[provider];
    else next[provider] = modes;
    onChange({ ...value, modes: next });
  };
  return (
    <View style={styles.permissionBlock} testID="bot-permissions">
      {BOT_TOOL_GROUPS.map((group) => (
        <View key={group} style={styles.permissionBlock}>
          <Checkbox
            checked={group === "observe" || value.groups.includes(group)}
            disabled={disabled || group === "observe"}
            label={t(GROUP_LABEL_KEYS[group])}
            onPress={() => toggleGroup(group)}
            testID={`bot-group-${group}`}
          />
          {RISKY_GROUPS.has(group) ? (
            <Text style={styles.hint}>{t("bots.permissionRiskHint")}</Text>
          ) : null}
          {group === "agents" ? (
            <Text style={styles.warning} testID="bot-command-reach-warning">
              {t("bots.commandReachWarning")}
            </Text>
          ) : null}
          {group === "terminals" ? (
            <Text style={styles.warning} testID="bot-terminals-warning">
              {t("bots.terminalsWarning")}
            </Text>
          ) : null}
        </View>
      ))}
      <Text style={styles.text}>{t("bots.modeCap")}</Text>
      <Text style={styles.hint}>{t("bots.modeCapHint")}</Text>
      <Text style={styles.warning} accessibilityRole="alert" testID="bot-mode-cap-warning">
        {t("bots.modeCapWarning")}
      </Text>
      {modeChoices(providers, value.modes, (provider) =>
        t("bots.providerUnavailable", { provider }),
      ).map((choices) => (
        <ProviderModes
          key={choices.provider}
          choices={choices}
          selected={value.modes[choices.provider] ?? []}
          disabled={disabled}
          onChange={(modes) => setModes(choices.provider, modes)}
        />
      ))}
    </View>
  );
}
