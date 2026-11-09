import { useRef, useState } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import { Download, Ellipsis, Play } from "lucide-react-native";
import type { ProviderUiState } from "@ait/protocol/messages";
import { Button } from "@/components/ui/button";
import { Combobox } from "@/components/ui/combobox";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { useFileDownload } from "@/hooks/use-file-download";
import { isWeb } from "@/constants/platform";
import { AgentControlTrigger } from "@/composer/agent-controls/control";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { ProviderNativeResult } from "./provider-native-result";
import { ProviderNativeField } from "./provider-native-field";
import {
  nativeActionArguments,
  nativeActionFields,
  nativeActionGroup,
  nativeActionValid,
  nativeOperationKey,
  type NativeArguments,
  type NativeGroup,
} from "./provider-native-action-model";

type Result = { text: string; file?: { path: string; fileName: string } | null };
type Outcome = { pending?: boolean; error?: string; result?: Result };

/** One task browser, with provider configuration rendered only inside provider settings. */
export function ProviderNativeActions({
  serverId,
  agentId,
  cwd,
  workspaceId,
  actions,
  disabled,
  onClose,
  surface = "composer",
  workers = [],
  turnActive = false,
  vibeEnabled = false,
}: {
  serverId: string;
  agentId: string;
  cwd: string;
  workspaceId?: string;
  actions: NonNullable<ProviderUiState["actions"]>;
  disabled: boolean;
  onClose?: () => void;
  surface?: "composer" | "settings";
  turnActive?: boolean;
  vibeEnabled?: boolean;
  workers?: NonNullable<ProviderUiState["vibe"]>["workers"];
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const download = useFileDownload({ serverId, workspaceId, workspaceRoot: cwd });
  const anchorRef = useRef<View>(null);
  const [open, setOpen] = useState(false);
  const [group, setGroup] = useState<NativeGroup | null>(null);
  const [actionId, setActionId] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  const [drafts, setDrafts] = useState<Record<string, NativeArguments>>({});
  const [ready, setReady] = useState<Record<string, boolean>>({});
  const [outcomes, setOutcomes] = useState<Record<string, Outcome>>({});
  const visibleActions = actions.filter((item) => {
    const category = nativeActionGroup(item.id);
    return (
      category &&
      (surface === "settings"
        ? ["accounts", "runtime"].includes(category)
        : !["accounts", "runtime"].includes(category))
    );
  });
  const action = visibleActions.find((item) => item.id === actionId);
  const values = drafts[actionId] ?? {};
  const fields = action ? nativeActionFields(action, values) : [];
  const operation = action ? nativeOperationKey(action, values) : null;
  const outcomeKey = `${actionId}:${operation ?? ""}`;
  const { pending = false, error, result } = outcomes[outcomeKey] ?? {};
  const changesHistory = ["new_session", "branch", "fork", "navigate_tree", "handoff"].includes(
    actionId,
  );
  const blockedReason =
    turnActive && (changesHistory || (actionId === "vibe_control" && operation === "toggle"))
      ? "finishTurn"
      : changesHistory && vibeEnabled
        ? "exitVibe"
        : null;
  const valid =
    !blockedReason &&
    action &&
    nativeActionValid(action, values) &&
    fields.every((field) => ready[`${actionId}:${field.id}`] !== false);
  const groups = (["conversation", "history", "tasks", "accounts", "runtime"] as const).filter(
    (category) => visibleActions.some((item) => nativeActionGroup(item.id) === category),
  );
  const setValue = (field: string, value: string) => {
    setDrafts((current) => ({
      ...current,
      [actionId]:
        field === "op" || field === "operation"
          ? { [field]: value }
          : {
              ...current[actionId],
              [field]: value,
              ...(field === "providerId" ? { credentialId: "" } : {}),
            },
    }));
  };
  const changeOpen = (next: boolean) => {
    setOpen(next);
    if (!next) {
      setActionId("");
      setGroup(null);
      setSearchQuery("");
      onClose?.();
    }
  };
  const runLabel = action
    ? t(`providerControl.operations.${operation ?? action.id}`, { defaultValue: action.label })
    : "";
  const run = async () => {
    if (!client || !action || pending || disabled || !valid) return;
    const key = outcomeKey;
    setOutcomes((current) => ({
      ...current,
      [key]: { ...current[key], pending: true, error: undefined },
    }));
    try {
      if (action.id === "get_btw_history") {
        await client.sendMessage(agentId, "/btw");
        setOutcomes((current) => ({ ...current, [key]: {} }));
        changeOpen(false);
        return;
      }
      const result = await client.runNativeAction(
        agentId,
        action.id,
        nativeActionArguments(action, values),
      );
      setOutcomes((current) => ({ ...current, [key]: { result } }));
    } catch (cause) {
      setOutcomes((current) => ({
        ...current,
        [key]: {
          ...current[key],
          pending: false,
          error: cause instanceof Error ? cause.message : String(cause),
        },
      }));
    }
  };
  const body = action ? (
    <View style={styles.form} testID="agent-action-form">
      {fields.map((field) => (
        <ProviderNativeField
          key={field.id}
          field={field}
          purpose={action.id}
          values={values}
          serverId={serverId}
          agentId={agentId}
          workers={workers.map((worker) => ({
            id: worker.id,
            label: worker.description || t("providerControl.tasks.worker"),
            description: worker.status,
          }))}
          onChange={(value) => setValue(field.id, value)}
          disabled={disabled || pending}
          onReady={(value) =>
            setReady((current) =>
              current[`${actionId}:${field.id}`] === value
                ? current
                : { ...current, [`${actionId}:${field.id}`]: value },
            )
          }
        />
      ))}
      {blockedReason ? (
        <Text style={styles.caption}>{t(`providerControl.tasks.${blockedReason}`)}</Text>
      ) : null}
      {error ? (
        <Text accessibilityRole="alert" style={styles.error}>
          {error}
        </Text>
      ) : null}
      {result ? (
        <ProviderNativeResult
          key={outcomeKey}
          text={result.text}
          fileName={result.file?.fileName}
        />
      ) : null}
    </View>
  ) : null;
  const footer = action ? (
    <View style={styles.footer}>
      <Button
        variant="outline"
        size="sm"
        leftIcon={Play}
        loading={pending}
        disabled={disabled || pending || !valid || !client}
        onPress={() => void run()}
        testID="native-action-submit"
      >
        {pending ? t("common.loading") : runLabel}
      </Button>
      {result?.file ? (
        <Button
          variant="outline"
          size="sm"
          leftIcon={Download}
          onPress={() => download(result.file!)}
        >
          {t("panels.file.downloadFile")}
        </Button>
      ) : null}
    </View>
  ) : null;
  if (!visibleActions.length) return null;
  return (
    <>
      {surface === "settings" ? (
        <View ref={anchorRef} style={styles.settings}>
          <Button variant="outline" size="sm" onPress={() => changeOpen(true)}>
            {t("providerControl.tasks.providerSettings")}
          </Button>
        </View>
      ) : (
        <Tooltip delayDuration={0} enabledOnDesktop enabledOnMobile={false}>
          <TooltipTrigger asChild triggerRefProp="ref">
            <AgentControlTrigger
              ref={anchorRef}
              icon={Ellipsis}
              surface="toolbar"
              label={t("providerControl.moreActions")}
              showToolbarLabel={false}
              open={open}
              disabled={disabled}
              onPress={() => changeOpen(!open)}
              accessibilityLabel={t("providerControl.moreActions")}
              accessibilityState={{ expanded: open }}
              aria-expanded={isWeb ? open : undefined}
              testID="agent-more-actions"
            />
          </TooltipTrigger>
          <TooltipContent side="top" align="center" offset={8}>
            <Text>{t("providerControl.moreActions")}</Text>
          </TooltipContent>
        </Tooltip>
      )}
      <Combobox
        anchorRef={anchorRef}
        open={open}
        onOpenChange={changeOpen}
        options={
          action
            ? []
            : group || searchQuery
              ? visibleActions
                  .filter(
                    (item) =>
                      (!group || nativeActionGroup(item.id) === group) &&
                      item.label.toLowerCase().includes(searchQuery.trim().toLowerCase()),
                  )
                  .map((item) => ({
                    id: item.id,
                    label: t(`providerControl.operations.${item.id}`, { defaultValue: item.label }),
                    description: group
                      ? undefined
                      : t(`providerControl.groups.${nativeActionGroup(item.id)}`),
                  }))
              : groups.map((id) => ({
                  id: `group:${id}`,
                  label: t(`providerControl.groups.${id}`),
                }))
        }
        value={actionId}
        onSelect={(id) => {
          if (id.startsWith("group:")) setGroup(id.slice(6) as NativeGroup);
          else setActionId(id);
          setSearchQuery("");
        }}
        keepOpenOnSelect
        desktopPlacement="top-start"
        desktopMinWidth={360}
        desktopPreventInitialFlash
        searchable={false}
        header={{
          title: action
            ? t(`providerControl.operations.${action.id}`, { defaultValue: action.label })
            : group
              ? t(`providerControl.groups.${group}`)
              : t("providerControl.moreActions"),
          ...(action || group
            ? {
                back: {
                  onPress: () => {
                    if (action) setActionId("");
                    else setGroup(null);
                    setSearchQuery("");
                  },
                  accessibilityLabel: t("common.actions.back"),
                },
              }
            : {}),
          ...(!action
            ? {
                search: {
                  onChange: setSearchQuery,
                  placeholder: t("common.actions.search"),
                  testID: "agent-actions-search",
                },
              }
            : {}),
        }}
        footer={footer}
      >
        {body}
      </Combobox>
    </>
  );
}

const styles = StyleSheet.create((theme) => ({
  form: { gap: 12, padding: 12 },
  settings: { alignSelf: "flex-start" },
  footer: { flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 8, padding: 12 },
  caption: { color: theme.colors.foregroundMuted, fontSize: 12 },
  error: { color: theme.colors.statusDanger, fontSize: 12 },
}));
