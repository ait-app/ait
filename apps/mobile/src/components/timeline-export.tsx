import { useState } from "react";
import { Pressable, Text, View } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import { useTranslation } from "react-i18next";
import type { AgentTimelineExportOptions } from "@ait/protocol/messages";
import { getHostRuntimeStore, useHostRuntimeSnapshot, useHosts } from "@/runtime/host-runtime";
import { useSessionStore } from "@/stores/session-store";
import { useDownloadStore } from "@/stores/download-store";

/** Export only committed display history; never invoke the provider or read runtime editor state. */
export function TimelineExport({ serverId, agentId }: { serverId: string; agentId: string }) {
  const { t } = useTranslation();
  const runtime = useHostRuntimeSnapshot(serverId);
  const hosts = useHosts();
  const supported = useSessionStore(
    (state) => state.sessions[serverId]?.serverInfo?.features?.agentTimelineExport === true,
  );
  const busy = useDownloadStore((state) =>
    Array.from(state.downloads.values()).some(
      (download) =>
        download.serverId === serverId &&
        download.scopeId === agentId &&
        download.status === "downloading",
    ),
  );
  const [format, setFormat] = useState<AgentTimelineExportOptions["format"]>("markdown");
  const [scope, setScope] = useState<AgentTimelineExportOptions["scope"]>("conversation");
  const available = supported && runtime?.connectionStatus === "online";
  const start = () => {
    const store = getHostRuntimeStore();
    const snapshot = store.getSnapshot(serverId);
    const client = store.getClient(serverId);
    if (
      !client ||
      snapshot?.connectionStatus !== "online" ||
      useSessionStore.getState().sessions[serverId]?.serverInfo?.features?.agentTimelineExport !==
        true ||
      Array.from(useDownloadStore.getState().downloads.values()).some(
        (download) =>
          download.serverId === serverId &&
          download.scopeId === agentId &&
          download.status === "downloading",
      )
    )
      return;
    void useDownloadStore.getState().startTimelineExport({
      serverId,
      agentId,
      daemonProfile: hosts.find((host) => host.serverId === serverId),
      activeConnectionId: snapshot.activeConnectionId,
      generate: async () => {
        const result = await client.exportAgentTimeline(agentId, { format, scope });
        const current = store.getSnapshot(serverId);
        if (
          store.getClient(serverId) !== client ||
          current?.connectionEpoch !== snapshot.connectionEpoch ||
          current?.clientGeneration !== snapshot.clientGeneration ||
          useSessionStore.getState().sessions[serverId]?.serverInfo?.features
            ?.agentTimelineExport !== true
        ) {
          await client.cancelAgentTimelineExport(agentId, result.token).catch(() => undefined);
          throw new Error(t("timelineExport.unavailable"));
        }
        return result;
      },
      release: (token) => client.cancelAgentTimelineExport(agentId, token),
    });
  };
  return (
    <View style={styles.section} testID="timeline-export">
      <Text style={styles.title}>{t("timelineExport.title")}</Text>
      <Text style={styles.detail}>{t("timelineExport.description")}</Text>
      {available ? (
        <>
          <View style={styles.choices}>
            {(["conversation", "timeline"] as const).map((value) => (
              <Pressable
                key={value}
                accessibilityRole="radio"
                accessibilityState={{ checked: scope === value }}
                onPress={() => setScope(value)}
                disabled={busy}
                style={[styles.choice, scope === value && styles.selected]}
              >
                <Text style={styles.text}>{t(`timelineExport.${value}`)}</Text>
              </Pressable>
            ))}
          </View>
          <View style={styles.choices}>
            {(["markdown", "html"] as const).map((value) => (
              <Pressable
                key={value}
                accessibilityRole="radio"
                accessibilityState={{ checked: format === value }}
                onPress={() => setFormat(value)}
                disabled={busy}
                style={[styles.choice, format === value && styles.selected]}
              >
                <Text style={styles.text}>
                  {value === "markdown" ? "Markdown (.md)" : "HTML (.html)"}
                </Text>
              </Pressable>
            ))}
          </View>
          <Pressable
            accessibilityRole="button"
            accessibilityState={{ disabled: busy }}
            disabled={busy}
            onPress={start}
            style={styles.choice}
            testID="timeline-export-download"
          >
            <Text style={styles.text}>{t("timelineExport.download")}</Text>
          </Pressable>
        </>
      ) : (
        <Text style={styles.detail}>{t("timelineExport.unavailable")}</Text>
      )}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  section: {
    gap: theme.spacing[2],
    borderTopWidth: 1,
    borderTopColor: theme.colors.borderAccent,
    paddingTop: theme.spacing[2],
  },
  title: { color: theme.colors.foreground, fontSize: theme.fontSize.base },
  detail: { color: theme.colors.foregroundMuted, fontSize: theme.fontSize.sm },
  text: { color: theme.colors.foreground, fontSize: theme.fontSize.sm },
  choices: { flexDirection: "row", flexWrap: "wrap", gap: theme.spacing[2] },
  choice: {
    borderWidth: 1,
    borderColor: theme.colors.border,
    borderRadius: theme.borderRadius.md,
    padding: theme.spacing[2],
  },
  selected: { borderColor: theme.colors.primary, backgroundColor: theme.colors.surface2 },
}));
