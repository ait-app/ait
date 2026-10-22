import { useState } from "react";
import { View, Text, Pressable, ScrollView } from "react-native";
import { router } from "expo-router";
import { useTranslation } from "react-i18next";
import { StyleSheet } from "react-native-unistyles";
import type { HubDirectoryItem } from "@ait/client/internal/hub";
import { useHubServices } from "@/runtime/hub-service-store";
import { useAccountState } from "@/runtime/account-state";
import type { HubDirectoryKind } from "@/runtime/hub-services";
import {
  buildHostWorkspaceRoute,
  buildHostAgentDetailRoute,
  buildProjectSettingsRoute,
} from "@/utils/host-routes";
import { SearchField } from "./ui/search-field";
import { Button } from "./ui/button";

/** Account summaries remain visible while a Host is offline; details always open on that Host. */
export function AccountDirectoryPanel() {
  const { t } = useTranslation();
  const { snapshot, directory, loading, error, refresh } = useHubServices();
  const hosts = useAccountState((state) => state.hosts);
  const [kind, setKind] = useState<HubDirectoryKind>("workspace");
  const [query, setQuery] = useState("");
  const [limit, setLimit] = useState(50);
  const supported = snapshot?.methods.includes(`${kind}.list.request`);
  const normalizedQuery = query.trim().toLocaleLowerCase();
  const items = directory[kind].filter((item) =>
    String(item.data.title ?? item.data.name ?? item.id)
      .toLocaleLowerCase()
      .includes(normalizedQuery),
  );
  const open = (item: HubDirectoryItem, serverId: string) => {
    if (kind === "project") router.navigate(buildProjectSettingsRoute(serverId, item.id));
    else if (kind === "workspace") router.navigate(buildHostWorkspaceRoute(serverId, item.id));
    else
      router.navigate(
        buildHostAgentDetailRoute(
          serverId,
          item.id,
          typeof item.data.workspaceId === "string" ? item.data.workspaceId : undefined,
        ),
      );
  };
  return (
    <View style={styles.panel} testID="account-directory">
      <Text style={styles.hint}>{t("onlineService.directoryHint")}</Text>
      <View style={styles.tabs}>
        {(["project", "workspace", "agent"] as const).map((value) => (
          <Button
            key={value}
            size="sm"
            variant={kind === value ? "default" : "outline"}
            onPress={() => {
              setKind(value);
              setLimit(50);
            }}
            accessibilityState={{ selected: kind === value }}
          >
            {t(`onlineService.directoryKinds.${value}`)}
          </Button>
        ))}
      </View>
      <SearchField
        value={query}
        onChangeText={(value) => {
          setQuery(value);
          setLimit(50);
        }}
        placeholder={t("onlineService.directorySearch")}
        clearAccessibilityLabel={t("sessions.actions.clearSearch")}
        testID="account-directory-search"
      />
      <Button
        variant="outline"
        disabled={loading || snapshot?.state !== "online" || !supported}
        loading={loading}
        onPress={() => void refresh().catch(() => {})}
      >
        {t("onlineService.refresh")}
      </Button>
      {!supported ? (
        <Text style={styles.hint}>{t("onlineService.directoryUnavailable")}</Text>
      ) : null}
      {error ? (
        <Text style={styles.hint} accessibilityRole="alert">
          {t("onlineService.directoryError")}
        </Text>
      ) : null}
      {supported && !loading && !error && items.length === 0 ? (
        <Text style={styles.hint}>{t("onlineService.directoryEmpty")}</Text>
      ) : null}
      <ScrollView style={styles.list} contentContainerStyle={styles.rows}>
        {items.slice(0, limit).map((item) => {
          const host = hosts.find((value) => value.host_id === item.hostId);
          const online = item.online && host?.online === true;
          const disabled = !online || !host?.server_id;
          return (
            <Pressable
              key={JSON.stringify([item.hostId, item.id])}
              accessibilityRole="button"
              accessibilityState={{ disabled }}
              disabled={disabled}
              testID={`account-directory-${kind}-${item.hostId}-${item.id}`}
              onPress={() => {
                if (host?.server_id) open(item, host.server_id);
              }}
              style={styles.row}
            >
              <Text style={styles.title} numberOfLines={1}>
                {String(item.data.title ?? item.data.name ?? item.id)}
              </Text>
              <Text style={styles.hint}>
                {host?.name ?? item.hostId} ·{" "}
                {t(
                  !online
                    ? "onlineService.directoryOffline"
                    : item.stale || snapshot?.state !== "online"
                      ? "onlineService.directoryStale"
                      : "onlineService.directoryOnline",
                )}
              </Text>
            </Pressable>
          );
        })}
        {items.length > limit ? (
          <Button variant="ghost" onPress={() => setLimit((value) => value + 50)}>
            {t("sessions.actions.loadMore")}
          </Button>
        ) : null}
      </ScrollView>
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  panel: { flex: 1, gap: theme.spacing[3], padding: theme.spacing[4] },
  tabs: { flexDirection: "row", flexWrap: "wrap", gap: theme.spacing[2] },
  hint: { color: theme.colors.foregroundMuted, fontSize: theme.fontSize.base },
  title: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
    fontWeight: theme.fontWeight.medium,
  },
  list: { flex: 1 },
  rows: { gap: theme.spacing[2] },
  row: {
    padding: theme.spacing[3],
    gap: theme.spacing[1],
    borderRadius: theme.borderRadius.lg,
    backgroundColor: theme.colors.surface2,
  },
}));
