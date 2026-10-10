import { HostStatusDot } from "@/components/host-status-dot";
import { Combobox, ComboboxItem, type ComboboxProps } from "@/components/ui/combobox";
import { useLocalDaemonServerId } from "@/hooks/use-is-local-daemon";
import { useHostRuntimeSnapshot, type ActiveConnection } from "@/runtime/host-runtime";
import { orderHostsLocalFirst } from "@/types/host-connection";
import type { TFunction } from "i18next";
import { Plus, Server, Settings } from "lucide-react-native";
import { useCallback, useMemo, type ReactElement, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { GestureResponderEvent } from "react-native";
import { Pressable, View } from "react-native";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import {
  ADD_HOST_OPTION_ID,
  ALL_HOSTS_OPTION_ID,
  ENABLE_BUILT_IN_DAEMON_OPTION_ID,
  getHostPickerLabel,
} from "./host-picker-constants";

export {
  ADD_HOST_OPTION_ID,
  ALL_HOSTS_OPTION_ID,
  ENABLE_BUILT_IN_DAEMON_OPTION_ID,
  getHostPickerLabel,
};

const SEARCHABLE_THRESHOLD = 10;
type RenderHostOption = NonNullable<ComboboxProps["renderOption"]>;
interface HostPickerHost {
  serverId: string;
  label: string;
}

export function HostStatusDotSlot({ serverId }: { serverId: string }): ReactElement {
  return (
    <View style={styles.statusDotSlot}>
      <HostStatusDot serverId={serverId} />
    </View>
  );
}

// Standard secure/plain web ports carry no information in the host display, so
// "server.example.com:443" reads as "server.example.com" while "127.0.0.1:6767" is kept.
function formatConnectionEndpoint(endpoint: string): string {
  return endpoint.replace(/:(?:443|80)$/, "");
}

// Socket/pipe transports have no host:port — their endpoint is a filesystem
// path, so they read as "Local". Online relays show their connection type;
// other transports show the address being used.
function formatActiveConnectionLabel(connection: ActiveConnection, t: TFunction): string {
  if (connection.type === "directSocket" || connection.type === "directPipe") {
    return t("hostPicker.local");
  }
  if (connection.type === "accountRelay") return t("settings.host.badges.relay");
  return formatConnectionEndpoint(connection.endpoint);
}

export interface HostPickerOptionProps {
  serverId: string;
  label: string;
  showActiveConnection: boolean;
  selected?: boolean;
  active: boolean;
  onPress: () => void;
  onOpenHostSettings?: (serverId: string) => void;
  testID?: string;
}

export function HostPickerOption({
  serverId,
  label,
  showActiveConnection,
  selected,
  active,
  onPress,
  onOpenHostSettings,
  testID,
}: HostPickerOptionProps): ReactElement {
  const { t } = useTranslation();
  const { theme } = useUnistyles();
  const activeConnection = useHostRuntimeSnapshot(serverId)?.activeConnection ?? null;
  const connectionLabel =
    showActiveConnection && activeConnection
      ? formatActiveConnectionLabel(activeConnection, t)
      : undefined;
  const leadingSlot = useMemo(() => <HostStatusDotSlot serverId={serverId} />, [serverId]);
  const handleSettingsPress = useCallback(
    (event: GestureResponderEvent) => {
      event.stopPropagation();
      onOpenHostSettings?.(serverId);
    },
    [onOpenHostSettings, serverId],
  );
  const trailingSlot = useMemo(() => {
    if (!onOpenHostSettings) return undefined;
    return (
      <Pressable
        onPress={handleSettingsPress}
        hitSlop={8}
        accessibilityRole="button"
        accessibilityLabel={t("hostPicker.settings", { host: label })}
      >
        <Settings size={theme.iconSize.sm} color={theme.colors.foregroundMuted} />
      </Pressable>
    );
  }, [
    handleSettingsPress,
    label,
    onOpenHostSettings,
    theme.colors.foregroundMuted,
    theme.iconSize.sm,
    t,
  ]);

  return (
    <ComboboxItem
      label={label}
      description={connectionLabel}
      leadingSlot={leadingSlot}
      trailingSlot={trailingSlot}
      selected={selected}
      active={active}
      onPress={onPress}
      testID={testID}
    />
  );
}

function SystemHostPickerOption({
  active,
  selected,
  onPress,
  kind,
  testID,
}: {
  active: boolean;
  selected?: boolean;
  onPress: () => void;
  kind: "add" | "all" | "enableBuiltInDaemon";
  testID?: string;
}): ReactElement {
  const { theme } = useUnistyles();
  const Icon = kind === "add" ? Plus : Server;
  const { t } = useTranslation();
  const label = t(`hostPicker.${kind}`);
  const leadingSlot = useMemo(
    () => <Icon size={theme.iconSize.sm} color={theme.colors.foregroundMuted} />,
    [Icon, theme.colors.foregroundMuted, theme.iconSize.sm],
  );

  return (
    <ComboboxItem
      label={label}
      leadingSlot={leadingSlot}
      selected={selected}
      active={active}
      onPress={onPress}
      testID={testID}
    />
  );
}

export interface HostPickerProps {
  hosts: HostPickerHost[];
  value: string;
  onSelect: (id: string) => void;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  anchorRef: React.RefObject<View | null>;
  includeAllHost?: boolean;
  includeAddHost?: boolean;
  onAddHost?: () => void;
  includeEnableBuiltInDaemon?: boolean;
  onEnableBuiltInDaemon?: () => void;
  showActiveConnection?: boolean;
  onOpenHostSettings?: (serverId: string) => void;
  searchable?: boolean;
  title?: string;
  desktopPlacement?: ComboboxProps["desktopPlacement"];
  desktopMinWidth?: number;
  addHostTestID?: string;
  hostOptionTestID?: (serverId: string) => string;
  children: ReactNode;
}

export function HostPicker({
  hosts,
  value,
  onSelect,
  open,
  onOpenChange,
  anchorRef,
  includeAllHost,
  includeAddHost,
  onAddHost,
  includeEnableBuiltInDaemon,
  onEnableBuiltInDaemon,
  showActiveConnection,
  onOpenHostSettings,
  searchable,
  title,
  desktopPlacement = "bottom-start",
  desktopMinWidth,
  addHostTestID,
  hostOptionTestID,
  children,
}: HostPickerProps): ReactElement {
  const { t } = useTranslation();
  const localServerId = useLocalDaemonServerId();
  const orderedHosts = useMemo(
    () => orderHostsLocalFirst(hosts, localServerId),
    [hosts, localServerId],
  );

  const options = useMemo(() => {
    const hostOptions = orderedHosts.map((host) => ({ id: host.serverId, label: host.label }));
    if (includeAllHost)
      hostOptions.unshift({ id: ALL_HOSTS_OPTION_ID, label: t("hostPicker.all") });
    if (includeAddHost) hostOptions.push({ id: ADD_HOST_OPTION_ID, label: t("hostPicker.add") });
    if (includeEnableBuiltInDaemon)
      hostOptions.push({
        id: ENABLE_BUILT_IN_DAEMON_OPTION_ID,
        label: t("hostPicker.enableBuiltInDaemon"),
      });
    return hostOptions;
  }, [orderedHosts, includeAllHost, includeAddHost, includeEnableBuiltInDaemon, t]);

  const isSearchable = searchable === true && orderedHosts.length > SEARCHABLE_THRESHOLD;

  const handleSelect = useCallback(
    (id: string) => {
      if (id === ADD_HOST_OPTION_ID) {
        onAddHost?.();
      } else if (id === ENABLE_BUILT_IN_DAEMON_OPTION_ID) {
        onEnableBuiltInDaemon?.();
      } else {
        onSelect(id);
      }
      onOpenChange(false);
    },
    [onAddHost, onEnableBuiltInDaemon, onOpenChange, onSelect],
  );

  const handleOpenHostSettings = useCallback(
    (serverId: string) => {
      onOpenHostSettings?.(serverId);
      onOpenChange(false);
    },
    [onOpenHostSettings, onOpenChange],
  );

  const renderOption = useCallback<RenderHostOption>(
    ({ option, selected, active, onPress }) => {
      if (option.id === ADD_HOST_OPTION_ID) {
        return (
          <SystemHostPickerOption
            kind="add"
            active={active}
            onPress={onPress}
            testID={addHostTestID}
          />
        );
      }
      if (option.id === ALL_HOSTS_OPTION_ID) {
        return (
          <SystemHostPickerOption
            kind="all"
            active={active}
            selected={selected}
            onPress={onPress}
            testID={hostOptionTestID?.(option.id)}
          />
        );
      }
      if (option.id === ENABLE_BUILT_IN_DAEMON_OPTION_ID) {
        return (
          <SystemHostPickerOption kind="enableBuiltInDaemon" active={active} onPress={onPress} />
        );
      }
      return (
        <HostPickerOption
          serverId={option.id}
          label={option.label}
          showActiveConnection={showActiveConnection === true}
          selected={selected}
          active={active}
          onPress={onPress}
          onOpenHostSettings={onOpenHostSettings ? handleOpenHostSettings : undefined}
          testID={hostOptionTestID?.(option.id)}
        />
      );
    },
    [
      addHostTestID,
      hostOptionTestID,
      onOpenHostSettings,
      showActiveConnection,
      handleOpenHostSettings,
    ],
  );

  return (
    <>
      {children}
      <Combobox
        options={options}
        value={value}
        onSelect={handleSelect}
        renderOption={renderOption}
        searchable={isSearchable}
        searchPlaceholder={t("hostPicker.search")}
        title={title ?? t("hostPicker.host")}
        open={open}
        onOpenChange={onOpenChange}
        anchorRef={anchorRef}
        desktopPlacement={desktopPlacement}
        desktopMinWidth={desktopMinWidth}
      />
    </>
  );
}

const styles = StyleSheet.create((theme) => ({
  statusDotSlot: {
    width: theme.iconSize.sm,
    height: theme.iconSize.sm,
    alignItems: "center",
    justifyContent: "center",
  },
}));
