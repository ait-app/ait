import React from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import type { ProviderSnapshotEntry } from "@ait/protocol/agent-types";
import { BOT_TOOL_GROUPS, type BotPermissions } from "@ait/protocol/bot/types";
import { getUnattendedModeId } from "@ait/protocol/provider-manifest";
import { BotCheckbox } from "./allow-groups-checkbox";
import { BotSelect } from "./bot-select";
import { botStyles as styles } from "./styles";

type GrantableGroup = (typeof BOT_TOOL_GROUPS)[number];

const GROUP_LABEL_KEYS: Record<GrantableGroup, string> = {
  observe: "bots.groupObserve",
  agents: "bots.groupAgents",
  stop: "bots.groupStop",
  schedules: "bots.groupSchedules",
  scripts: "bots.groupScripts",
  terminals: "bots.groupTerminals",
};

/** Groups whose tools run commands. */
const RISKY_GROUPS: ReadonlySet<GrantableGroup> = new Set(["scripts", "terminals"]);

/** Selector value for a provider the Bot may not start Agents with. */
const NOT_ALLOWED = "";

/** One provider's mode choices. */
interface ProviderModeChoices {
  provider: string;
  label: string;
  modes: readonly { id: string; label: string }[];
}

/**
 * Mode choices for each provider in the snapshot, followed by configured providers that are no
 * longer available, so they stay visible and can be cleared.
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
        modes: ids.slice(0, 1).map((id) => ({ id, label: id })),
      })),
  ];
}

function ProviderMode({
  choices,
  selected,
  disabled,
  onChange,
}: {
  choices: ProviderModeChoices;
  selected: string | undefined;
  disabled: boolean;
  onChange: (mode: string) => void;
}) {
  const { t } = useTranslation();
  const { provider } = choices;
  const unattended = getUnattendedModeId(provider);
  // A saved mode the provider stopped listing stays selectable, so nothing clears it by accident.
  const retired =
    selected && !choices.modes.some((mode) => mode.id === selected)
      ? [{ id: selected, label: selected }]
      : [];
  return (
    <BotSelect
      label={choices.label}
      value={selected ?? NOT_ALLOWED}
      options={[
        { id: NOT_ALLOWED, label: t("bots.modeNotAllowed") },
        ...choices.modes.map((mode) => ({
          id: mode.id,
          label: mode.id === unattended ? `⚠ ${mode.label}` : mode.label,
        })),
        ...retired,
      ]}
      onChange={onChange}
      disabled={disabled}
      testID={`bot-mode-${provider}`}
    />
  );
}

/** Tool-group switches and the per-provider Agent mode shared by setup and settings. */
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
  const toggleGroup = (group: GrantableGroup) => {
    const groups = value.groups.includes(group)
      ? value.groups.filter((id) => id !== group)
      : [...value.groups, group];
    onChange({ ...value, groups });
  };
  const setMode = (provider: string, mode: string) => {
    const next = { ...value.modes };
    if (mode === NOT_ALLOWED) delete next[provider];
    else next[provider] = [mode];
    onChange({ ...value, modes: next });
  };
  return (
    <View style={styles.permissionBlock} testID="bot-permissions">
      {BOT_TOOL_GROUPS.map((group) => (
        <View key={group} style={styles.permissionBlock}>
          <BotCheckbox
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
        <ProviderMode
          key={choices.provider}
          choices={choices}
          selected={value.modes[choices.provider]?.[0]}
          disabled={disabled}
          onChange={(mode) => setMode(choices.provider, mode)}
        />
      ))}
    </View>
  );
}
