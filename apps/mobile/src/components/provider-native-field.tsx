import { useEffect, useRef, useState } from "react";
import { Text, TextInput, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import { Button } from "@/components/ui/button";
import { SelectField } from "@/components/ui/select-field";
import { useHostRuntimeClient } from "@/runtime/host-runtime";
import {
  nativeObjectList,
  nativeObjectQuery,
  type NativeArguments,
  type NativeField,
  type NativeObjectOption,
} from "./provider-native-action-model";

const objectFields = new Set([
  "entryId",
  "subagentId",
  "recordId",
  "providerId",
  "credentialId",
  "sessionId",
  "sessions",
]);
const multilineFields = new Set([
  "prompt",
  "message",
  "objective",
  "question",
  "customInstructions",
  "instructions",
  "command",
]);

export function ProviderNativeField({
  field,
  purpose,
  values,
  onChange,
  disabled,
  serverId,
  agentId,
  workers,
  onReady,
}: {
  field: NativeField;
  purpose: string;
  values: NativeArguments;
  onChange: (value: string) => void;
  disabled: boolean;
  serverId: string;
  agentId: string;
  workers: NativeObjectOption[];
  onReady: (ready: boolean) => void;
}) {
  const { t } = useTranslation();
  const client = useHostRuntimeClient(serverId);
  const clientRef = useRef(client);
  clientRef.current = client;
  const [options, setOptions] = useState<NativeObjectOption[]>([]);
  const [omitted, setOmitted] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [revision, setRevision] = useState(0);
  const query = nativeObjectQuery(field.id, values);
  const provider = query?.arguments.providerId;
  const label = t(`providerControl.fields.${field.id}`, { defaultValue: field.label });
  const isWorker = field.id === "sessionId" || field.id === "sessions";
  const isObject = objectFields.has(field.id);
  const choices = isWorker
    ? workers
    : isObject
      ? options
      : field.options.map((id) => ({
          id,
          label: t(`providerControl.operations.${id}`, { defaultValue: id }),
        }));
  const value = values[field.id] ?? "";
  const selected =
    field.id === "sessions" ? value.split(",").filter(Boolean) : value ? [value] : [];
  const validSelection =
    !isObject ||
    (selected.every((id) => choices.some((item) => item.id === id)) &&
      !loading &&
      !error &&
      (isWorker || !!query));
  useEffect(() => {
    onReady(validSelection);
  }, [onReady, validSelection]);
  useEffect(() => {
    if (!isObject || isWorker) return;
    let active = true;
    const request = nativeObjectQuery(field.id, { providerId: provider ?? "" });
    setOptions([]);
    setOmitted(0);
    setError(null);
    if (!request || !clientRef.current) {
      setLoading(false);
      return;
    }
    setLoading(true);
    void clientRef.current
      .runNativeAction(agentId, request.action, request.arguments)
      .then((result) => {
        if (active) {
          const list = nativeObjectList(
            result.text,
            request.collection,
            label,
            purpose !== "login",
          );
          setOptions(
            list.options.filter(
              (item) => field.id !== "entryId" || purpose !== "branch" || item.role === "user",
            ),
          );
          setOmitted(list.omitted);
        }
      })
      .catch((cause: unknown) => {
        if (active)
          setError(
            cause instanceof SyntaxError
              ? t("providerControl.tasks.listIncomplete")
              : cause instanceof Error
                ? cause.message
                : String(cause),
          );
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [serverId, agentId, field.id, provider, revision, isObject, isWorker, purpose, label]);
  return (
    <View style={styles.field}>
      <Text style={styles.label}>
        {label}
        {field.required ? " *" : ""}
      </Text>
      {field.id === "sessions" ? (
        <View style={styles.choices}>
          <Text style={styles.caption}>{t("providerControl.tasks.waitAll")}</Text>
          {choices.map((item) => (
            <Button
              key={item.id}
              size="sm"
              variant={selected.includes(item.id) ? "secondary" : "outline"}
              disabled={disabled}
              onPress={() =>
                onChange(
                  (selected.includes(item.id)
                    ? selected.filter((id) => id !== item.id)
                    : [...selected, item.id]
                  ).join(","),
                )
              }
            >
              {item.label}
            </Button>
          ))}
        </View>
      ) : isObject || field.options.length ? (
        <SelectField
          field={false}
          size="sm"
          label={label}
          value={value || null}
          selectedDisplay={choices.find((item) => item.id === value) ?? null}
          options={choices.map((item) => ({ ...item, value: item.id }))}
          onChange={onChange}
          disabled={disabled || loading || !!error || (isObject && !choices.length)}
          searchable
          placeholder={loading ? t("common.loading") : t("common.actions.select")}
          emptyText={t("providerControl.tasks.noObjects")}
          testID={`native-field-${field.id}`}
        />
      ) : (
        <TextInput
          accessibilityLabel={label}
          value={value}
          editable={!disabled}
          onChangeText={onChange}
          multiline={multilineFields.has(field.id)}
          style={[styles.input, multilineFields.has(field.id) && styles.multiline]}
          testID={`native-field-${field.id}`}
        />
      )}
      {isObject && !isWorker ? (
        <View style={styles.choices}>
          {!loading && !error && !choices.length ? (
            <Text style={styles.caption}>{t("providerControl.tasks.noObjects")}</Text>
          ) : null}
          <Button
            variant="ghost"
            size="sm"
            disabled={disabled || loading || !query}
            onPress={() => setRevision((n) => n + 1)}
          >
            {t("common.actions.refresh")}
          </Button>
        </View>
      ) : null}
      {omitted > 0 ? (
        <Text style={styles.caption}>
          {t("providerControl.tasks.recentHistory", { count: omitted })}
        </Text>
      ) : null}
      {error ? (
        <Text accessibilityRole="alert" style={styles.error}>
          {error}
        </Text>
      ) : null}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  field: { gap: 6 },
  label: { color: theme.colors.foregroundMuted, fontSize: 12 },
  caption: { color: theme.colors.foregroundMuted, fontSize: 12 },
  choices: { flexDirection: "row", flexWrap: "wrap", gap: 6, alignItems: "center" },
  input: {
    borderWidth: 1,
    borderColor: theme.colors.border,
    borderRadius: 6,
    padding: 8,
    color: theme.colors.foreground,
  },
  multiline: { minHeight: 96, maxHeight: 200, textAlignVertical: "top" },
  error: { color: theme.colors.statusDanger, fontSize: 12 },
}));
