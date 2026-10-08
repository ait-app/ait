import React, { useRef, useState } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import { formatServerListen, parseServerListen } from "@ait/protocol/server-listen";
import { EditingTextInput } from "@/components/ui/text-input";
import { settingsStyles } from "@/styles/settings";

type ListenField = "host" | "port";

export function DaemonListenRows({
  listen,
  override,
  onChangeListen,
  disabled = false,
}: {
  listen: string;
  override: string | null;
  onChangeListen: (listen: string) => Promise<unknown>;
  disabled?: boolean;
}) {
  const { t } = useTranslation();
  const [fields, setFields] = useState(() => parseServerListen(listen));
  const draft = useRef(fields);
  const queue = useRef(Promise.resolve());
  const lastRequested = useRef<string | null>(listen);
  const [error, setError] = useState<{ field: ListenField; message: string } | null>(null);

  const change = (field: ListenField, value: string) => {
    draft.current = { ...draft.current, [field]: value };
    setFields(draft.current);
    setError(null);
  };

  const commit = (field: ListenField) => {
    if (disabled) return;
    let nextListen: string;
    try {
      nextListen = formatServerListen(draft.current.host, draft.current.port);
    } catch {
      setError({ field, message: t("desktop.daemon.listen.invalid") });
      return;
    }
    if (nextListen === lastRequested.current) return;
    lastRequested.current = nextListen;
    setError(null);
    // Serialize blur/Enter commits without blocking edits to the other field.
    queue.current = queue.current.then(async () => {
      try {
        await onChangeListen(nextListen);
        if (lastRequested.current === nextListen) setError(null);
      } catch {
        if (lastRequested.current !== nextListen) return;
        lastRequested.current = null;
        setError({ field, message: t("desktop.settings.saveFailed") });
      }
    });
  };

  return (
    <>
      {(["host", "port"] as const).map((field) => (
        <View key={field} style={[settingsStyles.row, settingsStyles.rowBorder]}>
          <View style={settingsStyles.rowContent}>
            <Text style={settingsStyles.rowTitle}>{t(`desktop.daemon.listen.${field}`)}</Text>
            {field === "host" && !override ? (
              <Text style={settingsStyles.rowHint}>{t("desktop.daemon.listen.savedHint")}</Text>
            ) : null}
            {field === "host" && override ? (
              <Text style={settingsStyles.rowHint}>
                {t("desktop.daemon.listen.override", { address: override })}
              </Text>
            ) : null}
            {error?.field === field ? (
              <Text style={settingsStyles.rowError} accessibilityRole="alert">
                {error.message}
              </Text>
            ) : null}
          </View>
          <EditingTextInput
            initialValue={fields[field]}
            onChangeText={(value) => change(field, value)}
            onBlur={() => commit(field)}
            onSubmitEditing={() => commit(field)}
            editable={!disabled}
            autoCapitalize="none"
            autoCorrect={false}
            spellCheck={false}
            keyboardType={field === "port" ? "number-pad" : "default"}
            selectTextOnFocus
            accessibilityLabel={t(`desktop.daemon.listen.${field}`)}
            testID={`server-listen-${field}`}
            style={[styles.input, field === "host" ? styles.hostInput : styles.portInput]}
          />
        </View>
      ))}
    </>
  );
}

const styles = StyleSheet.create((theme) => ({
  input: {
    minHeight: 36,
    paddingVertical: theme.spacing[2],
    paddingHorizontal: theme.spacing[3],
    borderRadius: theme.borderRadius.md,
    borderWidth: theme.borderWidth[1],
    borderColor: theme.colors.border,
    backgroundColor: theme.colors.surface2,
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
  },
  hostInput: { flexGrow: 1, flexShrink: 1, maxWidth: 280, textAlign: "left" },
  portInput: { width: 96, textAlign: "right" },
}));
