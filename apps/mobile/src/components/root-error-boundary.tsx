import React, { Component, type ErrorInfo, type ReactNode } from "react";
import { Text, View } from "react-native";
import { useTranslation } from "react-i18next";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { StyleSheet } from "react-native-unistyles";
import { Button } from "@/components/ui/button";
import { ScrollableCodeSurface } from "@/components/ui/scrollable-code-surface";
import { useIsCompactFormFactor } from "@/constants/layout";
import { formatCaughtValue } from "./root-error-details";
import { CopyErrorLogButton } from "./copy-error-log-button";
import { appErrorReporter, type AppErrorReport } from "@/diagnostics/error-reporter";
import { redactAppDiagnosticReport } from "@/diagnostics/app-diagnostic-report";

interface RootErrorBoundaryProps {
  children: ReactNode;
  onReload: () => void;
}

interface RootErrorBoundaryState {
  error: string | null;
  report?: AppErrorReport;
}

export class RootErrorBoundary extends Component<RootErrorBoundaryProps, RootErrorBoundaryState> {
  state: RootErrorBoundaryState = {
    error: null,
  };

  static getDerivedStateFromError(error: unknown): Partial<RootErrorBoundaryState> {
    return { error: redactAppDiagnosticReport(formatCaughtValue(error), []) };
  }

  componentDidCatch(error: unknown, errorInfo: ErrorInfo) {
    const report = appErrorReporter.report(error, {
      source: "React render",
      context: { componentStack: errorInfo.componentStack },
      notify: false,
    });
    this.setState({ report });
    appErrorReporter.presented(report);
    appErrorReporter.log("error", [
      "[RootErrorBoundary] Unhandled render error",
      report.details,
      report.context,
    ]);
  }

  render() {
    const { error } = this.state;
    if (error !== null) {
      return (
        <RootErrorFallback
          error={error}
          report={this.state.report}
          onReload={this.props.onReload}
        />
      );
    }

    return this.props.children;
  }
}

// A stack trace is reference material, not the screen. Cap it so the message and
// the reload action stay the shape of the page; the rest scrolls.
const DETAILS_MAX_HEIGHT = 300;

interface RootErrorFallbackProps {
  error: string;
  report?: AppErrorReport;
  onReload: () => void;
}

export function RootErrorFallback({ error, report, onReload }: RootErrorFallbackProps) {
  const { t } = useTranslation();
  const insets = useSafeAreaInsets();
  const isCompact = useIsCompactFormFactor();

  const reload = (
    <View style={styles.actions}>
      <CopyErrorLogButton report={report} error={error} source="React render" />
      <Button variant="default" onPress={onReload} testID="root-error-boundary-reload">
        {t("common.actions.reload")}
      </Button>
    </View>
  );

  return (
    <View
      style={[
        styles.container,
        {
          paddingTop: insets.top,
          paddingRight: insets.right,
          paddingBottom: insets.bottom,
          paddingLeft: insets.left,
        },
      ]}
      testID="root-error-boundary"
    >
      <View style={styles.content}>
        <View style={styles.header} testID="root-error-boundary-header">
          <Text style={styles.title}>{t("rootError.title")}</Text>
          <Text style={styles.body}>{t("rootError.body")}</Text>
        </View>
        <View style={styles.details}>
          <Text style={styles.detailsLabel}>{t("rootError.details")}</Text>
          <ScrollableCodeSurface
            maxHeight={isCompact ? undefined : DETAILS_MAX_HEIGHT}
            style={styles.detailsSurface}
            scrollStyle={styles.detailsScroll}
            testID="root-error-boundary-details"
          >
            {error}
          </ScrollableCodeSurface>
        </View>
        {isCompact ? null : reload}
      </View>
      {isCompact ? <View style={styles.footer}>{reload}</View> : null}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  actions: { gap: theme.spacing[3] },
  container: {
    flex: 1,
    backgroundColor: theme.colors.surface0,
  },
  content: {
    flex: 1,
    alignSelf: "center",
    width: "100%",
    maxWidth: 420,
    justifyContent: "center",
    gap: theme.spacing[6],
    paddingHorizontal: theme.spacing[6],
    paddingVertical: theme.spacing[8],
  },
  header: {
    alignItems: "center",
    gap: theme.spacing[2],
  },
  title: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
    fontWeight: theme.fontWeight.medium,
    textAlign: "center",
  },
  body: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.base,
    lineHeight: 20,
    textAlign: "center",
  },
  details: {
    flexShrink: 1,
    gap: theme.spacing[2],
  },
  detailsLabel: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
    fontWeight: theme.fontWeight.medium,
  },
  detailsSurface: {
    flexShrink: 1,
  },
  detailsScroll: {
    flexShrink: 1,
  },
  footer: {
    alignSelf: "center",
    width: "100%",
    maxWidth: 420,
    paddingHorizontal: theme.spacing[6],
    paddingBottom: theme.spacing[6],
  },
}));
