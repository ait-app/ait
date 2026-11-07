import { useRef, useState } from "react";
import { ScrollView, Text, TextInput, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import { ChevronDown, ChevronRight, Download, Play } from "lucide-react-native";
import type { ProviderUiState } from "@ait/protocol/messages";
import { Button } from "@/components/ui/button";
import { Combobox } from "@/components/ui/combobox";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import { useFileDownload } from "@/hooks/use-file-download";
import { useQueryClient } from "@tanstack/react-query";
import { refreshOmpModelsOnAccess } from "@/data/providers-snapshot";

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
}: {
  serverId: string;
  agentId: string;
  cwd: string;
  workspaceId?: string;
  actions: NonNullable<ProviderUiState["actions"]>;
  disabled: boolean;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const queryClient = useQueryClient();
  const download = useFileDownload({
    serverId,
    workspaceId,
    workspaceRoot: cwd,
  });
  const [expanded, setExpanded] = useState(false);
  const [actionId, setActionId] = useState(
    actions.find((action) => action.id === "get_session_stats")?.id ?? actions[0]?.id ?? "",
  );
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
    <View style={styles.container}>
      <Button
        variant="ghost"
        size="sm"
        leftIcon={expanded ? ChevronDown : ChevronRight}
        onPress={() => setExpanded(!expanded)}
      >
        <Text style={styles.text}>{t("providerControl.nativeOperations")}</Text>
      </Button>
      {expanded ? (
        <ScrollView style={styles.scroll} contentContainerStyle={styles.form}>
          <View style={styles.actions}>
            <ActionChoice
              value={actionId}
              options={actions.map((item) => ({
                id: item.id,
                label: item.label,
              }))}
              onSelect={setActionId}
            />
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
          {action?.fields.map((field) => (
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
                  disabled={pending}
                />
              ) : (
                <TextInput
                  accessibilityLabel={field.label}
                  value={arguments_[field.id] ?? ""}
                  editable={!pending}
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
            <Text selectable style={styles.text}>
              {result.text}
            </Text>
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
        </ScrollView>
      ) : null}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  container: { alignItems: "flex-start", gap: 6 },
  scroll: { maxHeight: 360, width: "100%" },
  form: { gap: 10, paddingBottom: 8 },
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
