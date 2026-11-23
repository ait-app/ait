import { Pressable, Text, View } from "react-native";
import { StyleSheet } from "react-native-unistyles";
import { useTranslation } from "react-i18next";
import {
  getHostRuntimeStore,
  isSharedResourceHost,
  useHostRuntimeSnapshot,
  useHosts,
} from "@/runtime/host-runtime";
import { useSessionStore } from "@/stores/session-store";
import { useDownloadStore } from "@/stores/download-store";

/**
 * Provider-native HTML export, separate from the generic saved-Timeline export. Only OMP
 * renders its own journal through the public native formatter; the bytes may contain active
 * scripts and upstream viewer resources, so this is a download only, never rendered inline.
 */
export function NativeHtmlExport({ serverId, agentId }: { serverId: string; agentId: string }) {
  const { t } = useTranslation();
  const runtime = useHostRuntimeSnapshot(serverId);
  const hosts = useHosts();
  const supported = useSessionStore(
    (state) => state.sessions[serverId]?.serverInfo?.features?.agentNativeHtmlExport === true,
  );
  const isOmp = useSessionStore(
    (state) => state.sessions[serverId]?.agents.get(agentId)?.provider === "omp",
  );
  const busy = useDownloadStore((state) =>
    Array.from(state.downloads.values()).some(
      (download) =>
        download.serverId === serverId &&
        download.scopeId === agentId &&
        download.status === "downloading",
    ),
  );
  const host = hosts.find((candidate) => candidate.serverId === serverId);
  if (!isOmp || (host && isSharedResourceHost(host))) return null;
  const available = Boolean(host) && supported && runtime?.connectionStatus === "online";
  const start = () => {
    const store = getHostRuntimeStore();
    const snapshot = store.getSnapshot(serverId);
    const client = store.getClient(serverId);
    const currentHost = store.getHosts().find((candidate) => candidate.serverId === serverId);
    if (
      !client ||
      !currentHost ||
      isSharedResourceHost(currentHost) ||
      snapshot?.connectionStatus !== "online" ||
      useSessionStore.getState().sessions[serverId]?.serverInfo?.features?.agentNativeHtmlExport !==
        true ||
      useSessionStore.getState().sessions[serverId]?.agents.get(agentId)?.provider !== "omp" ||
      Array.from(useDownloadStore.getState().downloads.values()).some(
        (download) =>
          download.serverId === serverId &&
          download.scopeId === agentId &&
          download.status === "downloading",
      )
    )
      return;
    void useDownloadStore.getState().startNativeHtmlExport({
      serverId,
      agentId,
      daemonProfile: currentHost,
      activeConnectionId: snapshot.activeConnectionId,
      generate: async (signal) => {
        const result = await client.exportAgentNativeHtml(agentId, { signal });
        const current = store.getSnapshot(serverId);
        const latestHost = store.getHosts().find((candidate) => candidate.serverId === serverId);
        if (
          !latestHost ||
          isSharedResourceHost(latestHost) ||
          store.getClient(serverId) !== client ||
          current?.connectionEpoch !== snapshot.connectionEpoch ||
          current?.clientGeneration !== snapshot.clientGeneration ||
          useSessionStore.getState().sessions[serverId]?.serverInfo?.features
            ?.agentNativeHtmlExport !== true
        ) {
          await client.cancelAgentNativeHtmlExport(agentId, result.token).catch(() => undefined);
          throw new Error(t("nativeHtmlExport.unavailable"));
        }
        return result;
      },
      release: (token) => client.cancelAgentNativeHtmlExport(agentId, token),
    });
  };
  return (
    <View style={styles.section} testID="native-html-export">
      <Text style={styles.title}>{t("nativeHtmlExport.title")}</Text>
      <Text style={styles.detail}>{t("nativeHtmlExport.description")}</Text>
      <Text style={styles.detail} testID="native-html-export-risk">
        {t("nativeHtmlExport.risk")}
      </Text>
      {available ? (
        <Pressable
          accessibilityRole="button"
          accessibilityState={{ disabled: busy }}
          disabled={busy}
          onPress={start}
          style={styles.choice}
          testID="native-html-export-download"
        >
          <Text style={styles.text}>{t("nativeHtmlExport.download")}</Text>
        </Pressable>
      ) : (
        <Text style={styles.detail}>{t("nativeHtmlExport.unavailable")}</Text>
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
  choice: {
    borderWidth: 1,
    borderColor: theme.colors.border,
    borderRadius: theme.borderRadius.md,
    padding: theme.spacing[2],
  },
}));
