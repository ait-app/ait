import React, { useEffect, useState } from "react";
import { Pressable, Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import {
  appErrorReporter,
  formatErrorReport,
  type AppErrorReport,
} from "@/diagnostics/error-reporter";
import { exportDiagnosticReport } from "@/diagnostics/export-diagnostic-report";
import { copyErrorLog } from "@/diagnostics/copy-error-log";

interface CopyErrorLogButtonProps {
  report?: AppErrorReport;
  error?: unknown;
  source?: string;
  context?: unknown;
}

/** Also works outside ToastProvider, including the root render-error fallback. */
export function CopyErrorLogButton({
  report,
  error,
  source = "inline error",
  context,
}: CopyErrorLogButtonProps) {
  const { t } = useTranslation();
  const [text, setText] = useState<string | null>(null);
  const [status, setStatus] = useState<"idle" | "copied" | "copyFailed" | "exportFailed">("idle");
  const [exporting, setExporting] = useState(false);
  const [localReport, setLocalReport] = useState<AppErrorReport | undefined>(report);

  useEffect(() => {
    let active = true;
    const current = report ?? appErrorReporter.report(error, { source, context, notify: false });
    setLocalReport(current);
    setText(formatErrorReport(current));
    setStatus("idle");
    // Lazy loading keeps runtime/store initialization out of the crash fallback's import path.
    void import("@/diagnostics/collect-error-report")
      .then(({ collectErrorReport }) => collectErrorReport(current))
      .then((value) => {
        if (active) setText(value);
      })
      .catch(() => {
        /* The original exception remains copyable when diagnostics are unavailable. */
      });
    return () => {
      active = false;
    };
  }, [report, error, source, context]);

  const reportText = text ?? (localReport ? formatErrorReport(localReport) : "");
  const copy = () => {
    if (!reportText) return;
    // Start clipboard access in the user's gesture; log collection has already run in the background.
    void copyErrorLog(reportText).then(
      () => setStatus("copied"),
      () => setStatus("copyFailed"),
    );
  };
  const exportLog = async () => {
    if (!reportText || exporting) return;
    setExporting(true);
    try {
      await exportDiagnosticReport(reportText);
    } catch {
      setStatus("exportFailed");
    } finally {
      setExporting(false);
    }
  };

  return (
    <View style={styles.container}>
      <View style={styles.actions}>
        <Pressable
          onPress={copy}
          accessibilityRole="button"
          accessibilityLabel={t("errorLog.copy")}
          testID="copy-error-log"
          style={styles.button}
        >
          <Text style={styles.label}>
            {t(status === "copied" ? "errorLog.copied" : "errorLog.copy")}
          </Text>
        </Pressable>
        <Pressable
          onPress={() => void exportLog()}
          disabled={exporting}
          accessibilityRole="button"
          accessibilityLabel={t("errorLog.export")}
          testID="export-error-log"
          style={styles.button}
        >
          <Text style={styles.label}>{t("errorLog.export")}</Text>
        </Pressable>
      </View>
      {status === "copyFailed" || status === "exportFailed" ? (
        <Text accessibilityRole="alert" style={styles.failure}>
          {t(`errorLog.${status}`)}
        </Text>
      ) : null}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  container: { gap: theme.spacing[1] },
  actions: { flexDirection: "row", flexWrap: "wrap", gap: theme.spacing[2] },
  button: {
    paddingVertical: theme.spacing[2],
    paddingHorizontal: theme.spacing[2],
    borderRadius: theme.borderRadius.md,
    backgroundColor: theme.colors.surface2,
  },
  label: { color: theme.colors.foreground, fontSize: theme.fontSize.sm },
  failure: { color: theme.colors.destructive, fontSize: theme.fontSize.sm },
}));
