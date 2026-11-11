import { Text, View } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import { useTranslation } from "react-i18next";
import { AgentUsage } from "@/usage";
import { formatTokenCount } from "./context-window-meter.utils";
import { SessionStatistics } from "./usage/session-statistics";
import { TimelineExport } from "./timeline-export";

interface ContextWindowDetailsProps {
  serverId: string;
  agentId: string;
  context: {
    percentage: number;
    usedTokens: number;
    maxTokens: number;
  } | null;
  /** The hover card and tooltip title themselves; the sheet's header carries the title instead. */
  showTitle: boolean;
  /** Whether the usage cards have a Refresh button; without one they show their freshness. */
  refreshable: boolean;
}

/**
 * What the context window meter opens: context fullness, the selected session's latest
 * provider statistics, then separate account quotas. Shared by hover, tooltip, and sheet.
 */
export function ContextWindowDetails({
  serverId,
  agentId,
  context,
  showTitle,
  refreshable,
}: ContextWindowDetailsProps) {
  const { t } = useTranslation();
  return (
    <>
      <View style={styles.summary}>
        {showTitle ? <Text style={styles.title}>{t("contextWindow.title")}</Text> : null}
        {context ? (
          <>
            <Text style={styles.text}>
              {t("contextWindow.used", { percentage: context.percentage })}
            </Text>
            <Text style={styles.detail}>
              {t("contextWindow.tokens", {
                used: formatTokenCount(context.usedTokens),
                max: formatTokenCount(context.maxTokens),
              })}
            </Text>
          </>
        ) : (
          <Text style={styles.detail}>{t("contextWindow.noData")}</Text>
        )}
      </View>
      <SessionStatistics serverId={serverId} agentId={agentId} />
      <TimelineExport serverId={serverId} agentId={agentId} />
      <Text style={styles.title}>{t("sessionStatistics.account")}</Text>
      <AgentUsage serverId={serverId} agentId={agentId} refreshable={refreshable} />
    </>
  );
}

const styles = StyleSheet.create((theme) => ({
  summary: {
    gap: theme.spacing[1.5],
    minWidth: 200,
  },
  title: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
  },
  text: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
    lineHeight: theme.fontSize.base * 1.4,
  },
  detail: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
    lineHeight: theme.fontSize.sm * 1.4,
  },
}));
