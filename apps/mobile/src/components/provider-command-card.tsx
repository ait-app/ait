import { useState } from "react";
import { Text, TextInput, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import {
  Check,
  ChevronDown,
  ChevronRight,
  Download,
  Pause,
  Pencil,
  Play,
  Target,
  Trash2,
  X,
} from "lucide-react-native";
import type { ProviderCommandTimelineItem } from "@ait/protocol/agent-types";
import { Button } from "@/components/ui/button";
import { StatusBadge } from "@/components/ui/status-badge";
import { useFileDownload } from "@/hooks/use-file-download";

import { ProviderNativeResult } from "./provider-native-result";

const actionIcons = { pause: Pause, resume: Play, edit: Pencil, clear: Trash2 };

export function ProviderCommandCard({
  result,
  active,
  onCommand,
  serverId,
  compact = false,
}: {
  result: ProviderCommandTimelineItem;
  active: boolean;
  onCommand?: (command: string) => Promise<void>;
  serverId?: string;
  compact?: boolean;
}) {
  const { t } = useTranslation();
  const { theme } = useUnistyles();
  const [expanded, setExpanded] = useState(false);
  const [editing, setEditing] = useState(false);
  const [objective, setObjective] = useState(result.goal?.objective ?? "");
  const [pending, setPending] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const goal = result.goal;
  const isGoal = result.goal !== undefined;
  const phase = result.status === "error" ? "error" : isGoal ? (goal?.phase ?? "empty") : "success";
  const status = t(`providerControl.status.${phase}`);
  const color =
    result.status === "error" ? theme.colors.statusDanger : theme.colors.foregroundMuted;
  const Icon = isGoal ? Target : result.status === "error" ? X : Check;
  const run = async (id: string, command: string) => {
    if (!onCommand || pending) return;
    setPending(id);
    setError(null);
    try {
      await onCommand(command);
      setEditing(false);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setPending(null);
    }
  };
  const editable = active && Boolean(onCommand);
  const editAction = result.actions?.find((action) => action.id === "edit");
  return (
    <View style={styles.card} testID="provider-command-card">
      <View style={styles.header}>
        <View style={styles.titleRow}>
          <Icon size={17} color={color} />
          <Text style={styles.title}>
            {isGoal
              ? t("providerControl.goal")
              : result.command.startsWith("native:")
                ? t(`providerControl.operations.${result.command.slice(7)}`, {
                    defaultValue: result.command.slice(7),
                  })
                : `/${result.command}`}
          </Text>
        </View>
        <StatusBadge
          label={status}
          variant={
            phase === "error" || phase === "blocked"
              ? "error"
              : phase === "complete" || phase === "success"
                ? "success"
                : "muted"
          }
        />
      </View>
      {goal ? (
        <View style={styles.body}>
          <Text style={styles.objective} numberOfLines={compact ? 4 : undefined} selectable>
            {goal.objective}
          </Text>
          {"rounds" in goal ? (
            <Text style={styles.caption}>
              {t("providerControl.rounds", { count: goal.rounds, max: goal.maxRounds })}
            </Text>
          ) : null}
          {"provider" in goal && typeof goal.tokensUsed === "number" ? (
            <Text style={styles.caption}>
              {t("providerControl.statistics.tokens")}: {goal.tokensUsed}
              {typeof goal.tokenBudget === "number" ? ` / ${goal.tokenBudget}` : ""}
            </Text>
          ) : null}
          {"blockedReason" in goal && goal.blockedReason ? (
            <Text style={styles.error}>{goal.blockedReason}</Text>
          ) : null}
        </View>
      ) : isGoal && result.status !== "error" ? (
        <Text style={styles.caption}>{t("providerControl.emptyGoal")}</Text>
      ) : null}
      {(!isGoal || result.status === "error") && result.text && !result.file ? (
        <ProviderNativeResult text={result.text} />
      ) : null}
      {result.file ? (
        <View style={styles.body}>
          <Text style={styles.caption} selectable>
            {result.file.fileName}
          </Text>
          {serverId ? <CommandFileDownload file={result.file} serverId={serverId} /> : null}
        </View>
      ) : null}
      {editing && editable ? (
        <View style={styles.body}>
          <TextInput
            accessibilityLabel={t("providerControl.editObjective")}
            value={objective}
            onChangeText={setObjective}
            multiline
            style={styles.input}
          />
          <View style={styles.actions}>
            <Button
              size="sm"
              disabled={!objective.trim() || pending !== null}
              loading={pending === "edit"}
              onPress={() => editAction && void run("edit", editAction.command + objective)}
            >
              {t("common.actions.save")}
            </Button>
            <Button
              size="sm"
              variant="ghost"
              disabled={pending !== null}
              onPress={() => setEditing(false)}
            >
              {t("common.actions.cancel")}
            </Button>
          </View>
        </View>
      ) : editable && result.actions?.length ? (
        <View style={styles.actions}>
          {result.actions.map((action) => (
            <Button
              key={action.id}
              size="sm"
              variant="ghost"
              leftIcon={actionIcons[action.id]}
              disabled={pending !== null}
              loading={pending === action.id}
              onPress={() =>
                action.id === "edit" ? setEditing(true) : void run(action.id, action.command)
              }
            >
              {t(`providerControl.actions.${action.id}`)}
            </Button>
          ))}
        </View>
      ) : null}
      {error ? (
        <Text style={styles.error} accessibilityRole="alert">
          {error}
        </Text>
      ) : null}
      {isGoal && result.text ? (
        <View>
          <Button
            size="xs"
            variant="ghost"
            leftIcon={expanded ? ChevronDown : ChevronRight}
            onPress={() => setExpanded(!expanded)}
            style={styles.detailsButton}
          >
            {t("providerControl.details")}
          </Button>
          {expanded ? (
            <Text style={styles.details} selectable>
              {result.text}
            </Text>
          ) : null}
        </View>
      ) : null}
    </View>
  );
}

function CommandFileDownload({
  file,
  serverId,
}: {
  file: NonNullable<ProviderCommandTimelineItem["file"]>;
  serverId: string;
}) {
  const { t } = useTranslation();
  const download = useFileDownload({ serverId, workspaceRoot: file.directory });
  return (
    <Button
      size="sm"
      variant="outline"
      leftIcon={Download}
      onPress={() => download({ fileName: file.fileName, path: file.fileName })}
    >
      {t("panels.file.downloadFile")}
    </Button>
  );
}

const styles = StyleSheet.create((theme) => ({
  card: {
    borderWidth: 1,
    borderColor: theme.colors.border,
    backgroundColor: theme.colors.surface1,
    borderRadius: theme.borderRadius.lg,
    padding: 16,
    gap: 12,
    maxWidth: 720,
    width: "100%",
  },
  header: { flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 12 },
  titleRow: { flexDirection: "row", alignItems: "center", gap: 8, flexShrink: 1 },
  title: { color: theme.colors.foreground, fontSize: 14, fontWeight: "600" },
  body: { gap: 8 },
  objective: { color: theme.colors.foreground, fontSize: 14, lineHeight: 22 },
  caption: { color: theme.colors.foregroundMuted, fontSize: 12, lineHeight: 18 },
  output: { color: theme.colors.foreground, fontSize: 13, lineHeight: 20 },
  actions: { flexDirection: "row", flexWrap: "wrap", gap: 6 },
  input: {
    color: theme.colors.foreground,
    backgroundColor: theme.colors.surface0,
    borderWidth: 1,
    borderColor: theme.colors.border,
    borderRadius: 8,
    padding: 10,
    minHeight: 88,
    fontSize: 14,
    lineHeight: 22,
  },
  error: { color: theme.colors.statusDanger, fontSize: 13, lineHeight: 20 },
  detailsButton: { alignSelf: "flex-start", paddingHorizontal: 0 },
  details: { color: theme.colors.foregroundMuted, fontSize: 12, lineHeight: 19, marginTop: 8 },
}));
