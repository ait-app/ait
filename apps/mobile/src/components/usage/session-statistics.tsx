import { Text, View } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import { useTranslation } from "react-i18next";
import { useFetchQuery } from "@/data/query";
import { getHostRuntimeStore, useHostRuntimeSnapshot } from "@/runtime/host-runtime";
import { useSessionStore } from "@/stores/session-store";

/** Read-only latest provider snapshot; never accumulate snapshots or include child usage. */
export function SessionStatistics({ serverId, agentId }: { serverId: string; agentId: string }) {
  const { t } = useTranslation();
  const runtime = useHostRuntimeSnapshot(serverId);
  const connected = runtime?.connectionStatus === "online";
  const supported = useSessionStore(
    (state) => state.sessions[serverId]?.serverInfo?.features?.agentStatisticsGet === true,
  );
  const query = useFetchQuery({
    queryKey: [
      "agent-statistics",
      serverId,
      agentId,
      runtime?.clientGeneration,
      runtime?.connectionEpoch,
    ],
    queryFn: async ({ signal }) => {
      // Recheck at dispatch as a connection may have downgraded since rendering.
      const session = useSessionStore.getState().sessions[serverId];
      const client = getHostRuntimeStore().getClient(serverId);
      if (!client || session?.serverInfo?.features?.agentStatisticsGet !== true) {
        throw new Error("Session statistics unavailable");
      }
      const statistics = await client.getAgentStatistics(agentId);
      signal.throwIfAborted();
      return statistics;
    },
    enabled: connected && supported,
    dataShape: "value",
    staleTimeMs: 0,
    retry: false,
  });
  const data = connected && supported && !query.isError ? query.data : undefined;
  return (
    <View style={styles.section} testID="session-statistics">
      <Text style={styles.title}>{t("sessionStatistics.title")}</Text>
      <Text style={styles.detail}>{t("sessionStatistics.scope")}</Text>
      <Text style={styles.detail}>{t("sessionStatistics.children")}</Text>
      {!data ? (
        <Text style={styles.detail} testID="session-statistics-state">
          {connected && supported && query.isPending
            ? t("providerUsage.loading")
            : t("sessionStatistics.unavailable")}
        </Text>
      ) : (
        <>
          {(
            [
              ["agentId", data.agentId],
              ["provider", data.provider],
              ["status", t(`agentList.status.${data.status}`)],
              ["configuredModel", data.configuredModel],
              ["effectiveModel", data.effectiveModel],
              ["updatedAt", data.updatedAt],
              ["inputTokens", data.usage.inputTokens],
              ["cachedInputTokens", data.usage.cachedInputTokens],
              ["outputTokens", data.usage.outputTokens],
              ["totalCostUsd", data.usage.totalCostUsd],
              ["contextWindowUsedTokens", data.usage.contextWindowUsedTokens],
              ["contextWindowMaxTokens", data.usage.contextWindowMaxTokens],
            ] as const
          ).map(([key, input]) => (
            <Text key={key} style={styles.detail} testID={`session-statistics-${key}`}>
              {t(`sessionStatistics.${key}`)}:{" "}
              {input === null ? t("providerUsage.unavailable") : String(input)}
            </Text>
          ))}
        </>
      )}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  section: {
    gap: theme.spacing[1.5],
    borderTopWidth: 1,
    borderTopColor: theme.colors.borderAccent,
    paddingTop: theme.spacing[2],
  },
  title: { color: theme.colors.foreground, fontSize: theme.fontSize.base },
  detail: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
    lineHeight: theme.fontSize.sm * 1.4,
  },
}));
