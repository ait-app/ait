import { useRef, useState } from "react";
import type { HostSnapshot } from "@ait/protocol/host-monitor";
import {
  Monitor,
  ChevronRight,
  ChevronDown,
  ArrowDown,
  ArrowUp,
  HardDrive,
  X,
} from "lucide-react-native";
import { Pressable, Text, View, useWindowDimensions } from "react-native";
import Svg, { Line, Path } from "react-native-svg";
import { useTranslation } from "react-i18next";
import { StyleSheet, useUnistyles } from "react-native-unistyles";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Combobox, ComboboxItem } from "@/components/ui/combobox";
import {
  bytes,
  historyPath,
  memoryPercent,
  networkHistory,
  percent,
  type MonitorHost,
} from "./model";

export interface HostMonitorViewProps {
  host: MonitorHost;
  hosts: readonly MonitorHost[];
  picked: string | null;
  onSelect: (serverId: string | null) => void;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  connected: boolean;
  supported: boolean;
  restricted: boolean;
  failed: boolean;
  snapshot?: HostSnapshot;
  onRetry: () => void;
}

export function HostMonitorView(props: HostMonitorViewProps) {
  const { host, snapshot, connected, supported, restricted, failed, open, onOpenChange } = props;
  const { t } = useTranslation();
  const { theme } = useUnistyles();
  const { height: viewportHeight } = useWindowDimensions();
  const current = snapshot?.current;
  const stale = !connected || failed || !supported || restricted;
  const state = !connected
    ? "offline"
    : restricted
      ? "restricted"
      : !supported
        ? "upgrade"
        : failed
          ? "failed"
          : !current
            ? "loading"
            : null;
  const statusColor = state ? theme.colors.foregroundMuted : theme.colors.statusDotSuccess;
  return (
    <DropdownMenu open={open} onOpenChange={onOpenChange} compactMode="sheet">
      <DropdownMenuTrigger
        accessibilityLabel={t("hostMonitor.title")}
        testID="sidebar-host-monitor"
        style={({ hovered, open: expanded }) => [
          styles.trigger,
          (hovered || expanded) && styles.triggerActive,
        ]}
      >
        <View style={styles.row}>
          <View style={[styles.dot, { backgroundColor: statusColor }]} />
          <Text style={styles.host} numberOfLines={1}>
            {host.label}
          </Text>
          <Text style={styles.small} numberOfLines={1}>
            {!connected
              ? t("hostMonitor.offline")
              : t(props.picked ? "hostMonitor.pinned" : "hostMonitor.follow")}
          </Text>
          <ChevronRight size={12} color={theme.colors.foregroundMuted} />
        </View>
        <View style={styles.summaryMetrics}>
          <SummaryMetric
            label="CPU"
            value={!stale && current ? current.cpuPercent : null}
            color={theme.colors.terminal.blue}
          />
          <View style={styles.summaryDivider} />
          <SummaryMetric
            label={t("hostMonitor.memory")}
            value={!stale && current ? memoryPercent(current) : null}
            color={theme.colors.foregroundMuted}
          />
        </View>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        side="right"
        align="end"
        offset={12}
        width={420}
        maxHeight={Math.max(240, viewportHeight - 32)}
        sheetTitle={t("hostMonitor.title")}
        scrollable
        testID="host-monitor-details"
      >
        <View style={styles.detail}>
          <View style={styles.identity}>
            <View style={styles.hostIcon}>
              <Monitor size={18} strokeWidth={1.5} color={theme.colors.foregroundMuted} />
            </View>
            <View style={styles.grow}>
              <Text style={styles.title} numberOfLines={1}>
                {host.label}
              </Text>
              <Text style={styles.caption} numberOfLines={1}>
                {snapshot
                  ? `${snapshot.platform} · ${t("hostMonitor.cores", { count: snapshot.cpuCount })}`
                  : t("hostMonitor.selectHost")}
              </Text>
            </View>
            <View style={styles.status}>
              <View style={[styles.dot, { backgroundColor: statusColor }]} />
              <Text style={styles.caption}>
                {t(state ? "hostMonitor.paused" : "hostMonitor.live")}
              </Text>
            </View>
            <Pressable
              onPress={() => onOpenChange(false)}
              accessibilityRole="button"
              accessibilityLabel={t("hostMonitor.close")}
              style={({ hovered }) => [styles.close, hovered && styles.triggerActive]}
            >
              <X size={15} color={theme.colors.foregroundMuted} />
            </Pressable>
          </View>
          {props.hosts.length > 1 ? (
            <HostSelection hosts={props.hosts} picked={props.picked} onSelect={props.onSelect} />
          ) : null}
          {state ? (
            <View style={styles.notice} testID="host-monitor-state">
              <Text style={[styles.caption, styles.grow]}>{t(`hostMonitor.${state}`)}</Text>
              {failed && connected && supported && !restricted ? (
                <Pressable accessibilityRole="button" onPress={props.onRetry}>
                  <Text style={styles.retry}>{t("hostMonitor.retry")}</Text>
                </Pressable>
              ) : null}
            </View>
          ) : null}
          {snapshot ? <HostDetails snapshot={snapshot} stale={stale} /> : null}
        </View>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function SummaryMetric({
  label,
  value,
  color,
}: {
  label: string;
  value: number | null;
  color: string;
}) {
  return (
    <View style={styles.summaryMetric}>
      <Text style={styles.caption}>{label} </Text>
      <Text style={styles.summaryValue}>{percent(value)}</Text>
      <View style={styles.miniTrack}>
        <View
          style={[
            styles.miniFill,
            { backgroundColor: color, width: `${Math.min(100, Math.max(0, value ?? 0))}%` },
          ]}
        />
      </View>
    </View>
  );
}

function HostSelection({
  hosts,
  picked,
  onSelect,
}: Pick<HostMonitorViewProps, "hosts" | "picked" | "onSelect">) {
  const { t } = useTranslation();
  const { theme } = useUnistyles();
  const anchorRef = useRef<View>(null);
  const [open, setOpen] = useState(false);
  return (
    <>
      <Pressable
        ref={anchorRef}
        onPress={() => setOpen(true)}
        accessibilityRole="button"
        accessibilityLabel={t("hostMonitor.selectHost")}
        style={styles.hostSelect}
      >
        <Text style={[styles.caption, styles.grow]} numberOfLines={1}>
          {hosts.find((host) => host.serverId === picked)?.label ??
            t("hostMonitor.followWorkspace")}
        </Text>
        <ChevronDown size={13} color={theme.colors.foregroundMuted} />
      </Pressable>
      <Combobox
        anchorRef={anchorRef}
        open={open}
        onOpenChange={setOpen}
        value={picked ?? "__follow__"}
        onSelect={(value) => {
          onSelect(value === "__follow__" ? null : value);
          setOpen(false);
        }}
        options={[
          { id: "__follow__", label: t("hostMonitor.followWorkspace") },
          ...hosts.map((host) => ({ id: host.serverId, label: host.label })),
        ]}
        title={t("hostMonitor.selectHost")}
        searchable={false}
        renderOption={({ option, selected, active, onPress }) => (
          <ComboboxItem
            label={option.label}
            selected={selected}
            active={active}
            onPress={onPress}
          />
        )}
      />
    </>
  );
}

function HostDetails({ snapshot, stale }: { snapshot: HostSnapshot; stale: boolean }) {
  const { t, i18n } = useTranslation();
  const { theme } = useUnistyles();
  const { current, history } = snapshot;
  const cpuColor = stale
    ? theme.colors.foregroundMuted
    : (current.cpuPercent ?? 0) >= 90
      ? theme.colors.destructive
      : theme.colors.terminal.blue;
  const memoryColor = theme.colors.foregroundMuted;
  return (
    <>
      <View style={styles.charts}>
        <View style={styles.metricCards}>
          <MetricCard
            label="CPU"
            value={percent(current.cpuPercent)}
            description={t("hostMonitor.utilization")}
            path={historyPath(history, "cpu")}
            chartLabel={t("hostMonitor.cpuHistory")}
            color={cpuColor}
          />
          <MetricCard
            label={t("hostMonitor.memory")}
            value={percent(memoryPercent(current))}
            description={`${bytes(current.memoryUsedBytes)} / ${bytes(current.memoryTotalBytes)}`}
            path={historyPath(history, "memory")}
            chartLabel={t("hostMonitor.memoryHistory")}
            color={memoryColor}
          />
        </View>
        <View style={styles.row}>
          <Text style={styles.small}>
            {t("hostMonitor.recentSeconds", {
              seconds: Math.max(
                0,
                Math.round(((history.at(-1)?.at ?? 0) - (history[0]?.at ?? 0)) / 1000),
              ),
            })}
          </Text>
          <View style={styles.grow} />
          <Text style={styles.small}>{t("hostMonitor.chartScale")}</Text>
        </View>
      </View>
      <NetworkChart snapshot={snapshot} />
      <View style={styles.section}>
        <View style={styles.row}>
          <HardDrive size={14} color={theme.colors.foregroundMuted} />
          <Text style={styles.sectionTitle}>{t("hostMonitor.disk")}</Text>
        </View>
        {snapshot.disks.length ? (
          snapshot.disks.map((disk) => (
            <View key={disk.mount} style={styles.disk}>
              <View style={styles.row}>
                <Text style={[styles.diskName, styles.grow]} numberOfLines={1}>
                  {disk.mount === "/"
                    ? t(
                        snapshot.platform === "macos"
                          ? "hostMonitor.apfs"
                          : "hostMonitor.systemDisk",
                      )
                    : disk.mount}
                </Text>
                <Text style={styles.caption}>
                  {t("hostMonitor.free", {
                    free: bytes(disk.availableBytes),
                    total: bytes(disk.totalBytes),
                  })}
                </Text>
              </View>
              <View style={styles.track}>
                <View
                  style={[
                    styles.fill,
                    {
                      width: `${Math.min(100, Math.max(0, disk.totalBytes ? (1 - disk.availableBytes / disk.totalBytes) * 100 : 0))}%`,
                    },
                  ]}
                />
              </View>
            </View>
          ))
        ) : (
          <Text style={styles.caption}>{t("hostMonitor.unavailable")}</Text>
        )}
      </View>
      <View style={styles.footer} testID="host-monitor-footer">
        <Text style={styles.small}>
          {t("hostMonitor.updated", {
            time: new Date(current.at).toLocaleTimeString(i18n.language, {
              hour: "2-digit",
              minute: "2-digit",
              second: "2-digit",
              hour12: false,
            }),
          })}
        </Text>
        {snapshot.uptimeSeconds !== null ? (
          <Text style={styles.small}>
            {t("hostMonitor.uptime", { hours: Math.floor(snapshot.uptimeSeconds / 3600) })}
          </Text>
        ) : null}
      </View>
    </>
  );
}

function MetricCard({
  label,
  value,
  description,
  path,
  chartLabel,
  color,
}: {
  label: string;
  value: string;
  description: string;
  path: string;
  chartLabel: string;
  color: string;
}) {
  const { t } = useTranslation();
  const { theme } = useUnistyles();
  return (
    <View style={styles.metricCard}>
      <View style={styles.cardHeading}>
        <View style={[styles.dot, { backgroundColor: color }]} />
        <Text style={styles.metricLabel}>{label}</Text>
      </View>
      <Text style={styles.metricValue}>
        {value.replace(/%$/, "")}
        <Text style={styles.metricUnit}>{value.endsWith("%") ? "%" : ""}</Text>
      </Text>
      <Text style={styles.metricDescription} numberOfLines={1}>
        {description}
      </Text>
      <View
        testID="host-monitor-history"
        style={styles.history}
        accessibilityRole="image"
        accessibilityLabel={chartLabel}
      >
        <Svg width="100%" height={32} viewBox="0 0 300 40" preserveAspectRatio="none">
          {[2, 20, 38].map((y) => (
            <Line
              key={y}
              x1={0}
              x2={300}
              y1={y}
              y2={y}
              stroke={theme.colors.border}
              strokeWidth={0.7}
              strokeDasharray="4 6"
            />
          ))}
          {path.includes("L") ? (
            <Path
              d={path}
              fill="none"
              stroke={color}
              strokeWidth={2}
              strokeLinejoin="round"
              strokeLinecap="round"
            />
          ) : null}
        </Svg>
        {!path.includes("L") ? (
          <View style={styles.collecting}>
            <Text style={styles.small}>{t("hostMonitor.collecting")}</Text>
          </View>
        ) : null}
      </View>
    </View>
  );
}

function NetworkChart({ snapshot }: { snapshot: HostSnapshot }) {
  const { t } = useTranslation();
  const { theme } = useUnistyles();
  const chart = networkHistory(snapshot.history);
  const downloadColor = theme.colors.terminal.blue;
  const uploadColor = theme.colors.terminal.magenta;
  const ready = chart.download.includes("L") || chart.upload.includes("L");
  return (
    <View style={styles.section}>
      <View style={styles.row}>
        <Text style={styles.sectionTitle}>{t("hostMonitor.network")}</Text>
        <View style={styles.grow} />
        <Text style={styles.small}>0–{bytes(chart.maximum)}/s</Text>
      </View>
      <View style={styles.networkMetrics}>
        <NetworkMetric
          direction="down"
          label={t("hostMonitor.download")}
          value={snapshot.current.rxBytesPerSecond}
          color={downloadColor}
        />
        <NetworkMetric
          direction="up"
          label={t("hostMonitor.upload")}
          value={snapshot.current.txBytesPerSecond}
          color={uploadColor}
        />
      </View>
      <View
        style={styles.networkHistory}
        testID="host-monitor-network-history"
        accessibilityRole="image"
        accessibilityLabel={t("hostMonitor.networkHistory")}
      >
        <Svg width="100%" height={44} viewBox="0 0 300 40" preserveAspectRatio="none">
          {[2, 20, 38].map((y) => (
            <Line
              key={y}
              x1={0}
              x2={300}
              y1={y}
              y2={y}
              stroke={theme.colors.border}
              strokeWidth={0.7}
              strokeDasharray="4 6"
            />
          ))}
          {chart.download.includes("L") ? (
            <Path
              d={chart.download}
              fill="none"
              stroke={downloadColor}
              strokeWidth={1.5}
              strokeLinejoin="round"
            />
          ) : null}
          {chart.upload.includes("L") ? (
            <Path
              d={chart.upload}
              fill="none"
              stroke={uploadColor}
              strokeWidth={1.5}
              strokeDasharray="4 3"
              strokeLinejoin="round"
            />
          ) : null}
        </Svg>
        {!ready ? (
          <View style={styles.collecting}>
            <Text style={styles.small}>{t("hostMonitor.collecting")}</Text>
          </View>
        ) : null}
      </View>
    </View>
  );
}

function NetworkMetric({
  direction,
  label,
  value,
  color,
}: {
  direction: "up" | "down";
  label: string;
  value: number | null;
  color: string;
}) {
  const Icon = direction === "down" ? ArrowDown : ArrowUp;
  return (
    <View style={styles.networkMetric}>
      <Icon size={12} color={color} />
      <Text style={styles.caption}>{label}</Text>
      <Text style={styles.networkValue}>
        {bytes(value)}
        <Text style={styles.rateUnit}>{value === null ? "" : "/s"}</Text>
      </Text>
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  trigger: {
    gap: 9,
    paddingVertical: 10,
    paddingHorizontal: 10,
    borderRadius: 8,
    marginHorizontal: 6,
    marginVertical: 5,
  },
  triggerActive: { backgroundColor: theme.colors.surfaceSidebarHover },
  row: { flexDirection: "row", alignItems: "center", gap: 8, minWidth: 0 },
  grow: { flex: 1, minWidth: 0 },
  dot: { width: 6, height: 6, borderRadius: 3, flexShrink: 0 },
  host: { flex: 1, color: theme.colors.foreground, fontSize: theme.fontSize.sm, fontWeight: "500" },
  caption: { color: theme.colors.foregroundMuted, fontSize: theme.fontSize.sm },
  small: { color: theme.colors.foregroundMuted, fontSize: theme.fontSize.sm - 1 },
  summaryMetrics: { flexDirection: "row", alignItems: "center", gap: 12 },
  summaryMetric: { flex: 1, flexDirection: "row", alignItems: "center", gap: 3, minWidth: 0 },
  summaryValue: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.sm,
    fontVariant: ["tabular-nums"],
  },
  summaryDivider: { height: 12, width: 1, backgroundColor: theme.colors.border },
  miniTrack: {
    flex: 1,
    maxWidth: 32,
    height: 3,
    backgroundColor: theme.colors.surface3,
    borderRadius: 2,
    marginLeft: 5,
    overflow: "hidden",
  },
  miniFill: { height: 3, borderRadius: 2 },
  detail: { padding: 14, gap: 10 },
  status: { flexDirection: "row", alignItems: "center", gap: 5 },
  close: { padding: 5, borderRadius: 6, marginRight: -5 },
  identity: { flexDirection: "row", alignItems: "center", gap: 10 },
  hostIcon: {
    width: 36,
    height: 36,
    borderRadius: 10,
    backgroundColor: theme.colors.surface2,
    alignItems: "center",
    justifyContent: "center",
  },
  title: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.lg,
    fontWeight: "600",
    marginBottom: 3,
  },
  hostSelect: {
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
    backgroundColor: theme.colors.surface2,
    borderRadius: 6,
    paddingVertical: 7,
    paddingHorizontal: 10,
    marginTop: -6,
  },
  notice: {
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
    padding: 10,
    backgroundColor: theme.colors.surface2,
    borderRadius: 8,
  },
  retry: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.sm,
    textDecorationLine: "underline",
  },
  charts: { gap: 4 },
  metricCards: { flexDirection: "row", gap: 10 },
  metricCard: {
    flex: 1,
    minWidth: 0,
    padding: 10,
    borderRadius: 10,
    borderWidth: 1,
    borderColor: theme.colors.border,
    backgroundColor: theme.colors.surface0,
  },
  cardHeading: { flexDirection: "row", alignItems: "center", gap: 6 },
  metricLabel: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm,
    fontWeight: "500",
  },
  metricValue: {
    color: theme.colors.foreground,
    fontSize: 28,
    lineHeight: 34,
    fontWeight: "500",
    letterSpacing: -1,
    marginTop: 2,
    fontVariant: ["tabular-nums"],
  },
  metricUnit: {
    fontSize: 18,
    color: theme.colors.foregroundMuted,
    fontWeight: "400",
    letterSpacing: 0,
  },
  metricDescription: {
    color: theme.colors.foregroundMuted,
    fontSize: theme.fontSize.sm - 1,
    marginTop: 1,
  },
  history: { height: 32, marginTop: 8, overflow: "hidden" },
  collecting: {
    position: "absolute",
    top: 0,
    bottom: 0,
    left: 0,
    right: 0,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: theme.colors.surface0,
  },
  section: { borderTopWidth: 1, borderColor: theme.colors.border, paddingTop: 10, gap: 8 },
  sectionTitle: { color: theme.colors.foreground, fontSize: theme.fontSize.sm, fontWeight: "500" },
  networkMetrics: { flexDirection: "row", gap: 18 },
  networkMetric: {
    flex: 1,
    minWidth: 0,
    flexDirection: "row",
    alignItems: "center",
    gap: 4,
    flexWrap: "wrap",
  },
  networkHistory: { height: 44, overflow: "hidden" },
  networkValue: {
    color: theme.colors.foreground,
    fontSize: theme.fontSize.base,
    fontWeight: "500",
    fontVariant: ["tabular-nums"],
  },
  rateUnit: { color: theme.colors.foregroundMuted, fontSize: theme.fontSize.sm, fontWeight: "400" },
  disk: { gap: 5 },
  diskName: { color: theme.colors.foreground, fontSize: theme.fontSize.sm },
  track: { height: 5, borderRadius: 3, backgroundColor: theme.colors.surface2, overflow: "hidden" },
  fill: { height: 5, borderRadius: 3, backgroundColor: theme.colors.foregroundMuted, opacity: 0.7 },
  footer: {
    borderTopWidth: 1,
    borderColor: theme.colors.border,
    paddingTop: 8,
    flexDirection: "row",
    justifyContent: "space-between",
    flexWrap: "wrap",
    gap: 4,
  },
}));
