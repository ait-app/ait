import { useRef, useState } from "react";
import { Text, TextInput, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import { ChevronDown, Download, Ellipsis, Play } from "lucide-react-native";
import type { ProviderUiState } from "@ait/protocol/messages";
import { Button } from "@/components/ui/button";
import { Combobox } from "@/components/ui/combobox";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { useFileDownload } from "@/hooks/use-file-download";
import { useQueryClient } from "@tanstack/react-query";
import { refreshOmpModelsOnAccess } from "@/data/providers-snapshot";
import { AgentControlTrigger } from "@/composer/agent-controls/control";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

import { ProviderNativeResult } from "./provider-native-result";

function ActionChoice({
  value,
  options,
  onSelect,
  disabled = false,
}: {
  value: string;
  options: { id: string; label: string }[];
  onSelect: (id: string) => void;
  disabled?: boolean;
}) {
  const { t } = useTranslation();
  const anchor = useRef<View>(null);
  const [open, setOpen] = useState(false);
  return (
    <View ref={anchor} style={styles.choice}>
      <Button
        variant="outline"
        size="sm"
        trailing={<ChevronDown size={12} color={styles.text.color} />}
        disabled={disabled}
        onPress={() => setOpen(true)}
      >
        <Text style={styles.text} numberOfLines={1}>
          {options.find((option) => option.id === value)?.label ?? t("common.actions.select")}
        </Text>
      </Button>
      <Combobox
        value={value}
        options={options}
        onSelect={onSelect}
        anchorRef={anchor}
        open={open}
        onOpenChange={setOpen}
        searchable
      />
    </View>
  );
}

export function ProviderNativeActions({
  serverId,
  agentId,
  cwd,
  workspaceId,
  actions,
  disabled,
  onClose,
}: {
  serverId: string;
  agentId: string;
  cwd: string;
  workspaceId?: string;
  actions: NonNullable<ProviderUiState["actions"]>;
  disabled: boolean;
  onClose?: () => void;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const queryClient = useQueryClient();
  const download = useFileDownload({
    serverId,
    workspaceId,
    workspaceRoot: cwd,
  });
  const anchorRef = useRef<View>(null);
  const [open, setOpen] = useState(false);
  const [actionId, setActionId] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  const [drafts, setDrafts] = useState<Record<string, Record<string, string>>>({});
  type Result = {
    text: string;
    file?: { path: string; fileName: string } | null;
  };
  const [outcomes, setOutcomes] = useState<
    Record<string, { pending?: boolean; error?: string; result?: Result }>
  >({});
  const arguments_ = drafts[actionId] ?? {};
  const { pending = false, error, result } = outcomes[actionId] ?? {};
  const setArguments = (update: (current: Record<string, string>) => Record<string, string>) =>
    setDrafts((current) => ({
      ...current,
      [actionId]: update(current[actionId] ?? {}),
    }));
  const action = actions.find((candidate) => candidate.id === actionId);
  const handleOpenChange = (next: boolean) => {
    setOpen(next);
    if (!next) {
      setActionId("");
      setSearchQuery("");
      onClose?.();
    }
  };
  const missing = action?.fields.some((field) => field.required && !arguments_[field.id]?.trim());
  const run = async () => {
    if (!client || !action || pending || disabled || missing) return;
    const id = action.id;
    setOutcomes((current) => ({
      ...current,
      [id]: { ...current[id], pending: true, error: undefined },
    }));
    try {
      const result = await client.runNativeAction(agentId, id, arguments_);
      setOutcomes((current) => ({ ...current, [id]: { result } }));
    } catch (cause) {
      const error = cause instanceof Error ? cause.message : String(cause);
      setOutcomes((current) => ({
        ...current,
        [id]: { ...current[id], pending: false, error },
      }));
    } finally {
      if (id === "model_preset_save" || id === "model_preset_delete") {
        void refreshOmpModelsOnAccess({
          client,
          queryClient,
          serverId,
          cwd,
        }).catch((cause) => {
          const error = cause instanceof Error ? cause.message : String(cause);
          setOutcomes((current) => ({
            ...current,
            [id]: { ...current[id], error },
          }));
        });
      }
    }
  };
  return (
    <>
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
            onPress={() => handleOpenChange(!open)}
            accessibilityLabel={t("providerControl.moreActions")}
            accessibilityState={{ expanded: open }}
            testID="agent-more-actions"
          />
        </TooltipTrigger>
        <TooltipContent side="top" align="center" offset={8}>
          <Text>{t("providerControl.moreActions")}</Text>
        </TooltipContent>
      </Tooltip>
      <Combobox
        anchorRef={anchorRef}
        open={open}
        onOpenChange={handleOpenChange}
        options={
          action
            ? []
            : actions
                .filter((item) =>
                  item.label.toLowerCase().includes(searchQuery.trim().toLowerCase()),
                )
                .map((item) => ({ id: item.id, label: item.label }))
        }
        value={actionId}
        onSelect={(id) => {
          setActionId(id);
          setSearchQuery("");
        }}
        keepOpenOnSelect
        desktopPlacement="top-start"
        desktopMinWidth={320}
        desktopPreventInitialFlash
        searchable={false}
        header={{
          title: action?.label ?? t("providerControl.moreActions"),
          ...(action
            ? {
                back: {
                  onPress: () => setActionId(""),
                  accessibilityLabel: t("common.actions.back"),
                },
              }
            : {
                search: {
                  onChange: setSearchQuery,
                  placeholder: t("common.actions.search"),
                  testID: "agent-actions-search",
                },
              }),
        }}
      >
        {action ? (
          <View style={styles.form} testID="agent-action-form">
            <View style={styles.actions}>
              <Button
                variant="outline"
                size="sm"
                leftIcon={Play}
                loading={pending}
                disabled={disabled || pending || missing || !client}
                onPress={() => void run()}
              >
                {pending ? t("common.loading") : t("providerControl.runOperation")}
              </Button>
            </View>
            {action.fields.map((field) => (
              <View key={field.id} style={styles.field}>
                <Text style={styles.text}>{field.label}</Text>
                {field.options.length ? (
                  <ActionChoice
                    value={arguments_[field.id] ?? ""}
                    options={field.options.map((value) => ({
                      id: value,
                      label: value,
                    }))}
                    onSelect={(value) =>
                      !pending &&
                      setArguments((current) => ({
                        ...current,
                        [field.id]: value,
                      }))
                    }
                    disabled={disabled || pending}
                  />
                ) : (
                  <TextInput
                    accessibilityLabel={field.label}
                    value={arguments_[field.id] ?? ""}
                    editable={!disabled && !pending}
                    style={styles.input}
                    onChangeText={(value) =>
                      setArguments((current) => ({
                        ...current,
                        [field.id]: value,
                      }))
                    }
                  />
                )}
              </View>
            ))}
            {error ? (
              <Text accessibilityRole="alert" style={styles.error}>
                {error}
              </Text>
            ) : null}
            {result ? (
              <ProviderNativeResult
                key={actionId}
                text={result.text}
                fileName={result.file?.fileName}
              />
            ) : null}
            {result?.file ? (
              <View style={styles.actions}>
                <Button
                  variant="outline"
                  size="sm"
                  leftIcon={Download}
                  onPress={() => download(result.file!)}
                >
                  {t("panels.file.downloadFile")}
                </Button>
              </View>
            ) : null}
          </View>
        ) : null}
      </Combobox>
    </>
  );
}

const styles = StyleSheet.create((theme) => ({
  form: { gap: 10, padding: 12 },
  actions: {
    flexDirection: "row",
    flexWrap: "wrap",
    alignItems: "center",
    gap: 8,
  },
  choice: { alignSelf: "flex-start", maxWidth: "100%", flexShrink: 1 },
  field: { gap: 6 },
  text: { color: theme.colors.foregroundMuted, fontSize: 12, lineHeight: 18 },
  error: { color: theme.colors.statusDanger, fontSize: 12 },
  input: {
    borderWidth: 1,
    borderColor: theme.colors.border,
    borderRadius: 6,
    padding: 8,
    color: theme.colors.foreground,
  },
}));
