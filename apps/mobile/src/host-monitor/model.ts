import type { HostSample, HostSnapshot } from "@ait/protocol/host-monitor";

export interface MonitorHost {
  serverId: string;
  label: string;
}

/** Call with the two caches for one host; ties keep the detail snapshot's history. */
export function newestHostSnapshot(summary?: HostSnapshot, detail?: HostSnapshot) {
  if (!summary) return detail;
  if (!detail) return summary;
  return summary.current.at > detail.current.at ? summary : detail;
}

/** Keep an offline selected host visible; never silently substitute another machine. */
export function resolveMonitorHost(
  hosts: readonly MonitorHost[],
  picked: string | null,
  active: string | null,
): MonitorHost | null {
  return (
    hosts.find((host) => host.serverId === picked) ??
    hosts.find((host) => host.serverId === active) ??
    hosts[0] ??
    null
  );
}

export function percent(value: number | null): string {
  if (value === null || !Number.isFinite(value)) return "—";
  const bounded = Math.min(100, Math.max(0, value));
  return `${bounded < 10 ? Number(bounded.toFixed(1)) : Math.round(bounded)}%`;
}

export function memoryPercent(sample: HostSample): number | null {
  return sample.memoryTotalBytes > 0
    ? (sample.memoryUsedBytes / sample.memoryTotalBytes) * 100
    : null;
}

export function bytes(value: number | null): string {
  if (value === null) return "—";
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let amount = Math.max(value, 0),
    index = 0;
  while (amount >= 1024 && index < units.length - 1) {
    amount /= 1024;
    index++;
  }
  return `${index === 0 ? Math.round(amount) : Number(amount.toFixed(1))} ${units[index]}`;
}

/** Plot actual timestamps, breaking the path after pauses instead of inventing continuity. */
export function historyPath(
  history: readonly HostSample[],
  metric: "cpu" | "memory" | "download" | "upload",
  maximum = 100,
): string {
  const end = history.at(-1)?.at;
  if (end === undefined) return "";
  const start = Math.max(end - 180_000, history[0]?.at ?? end);
  const duration = Math.max(2_000, end - start);
  let previous: number | null = null;
  return history
    .flatMap((sample) => {
      const value =
        metric === "cpu"
          ? sample.cpuPercent
          : metric === "memory"
            ? memoryPercent(sample)
            : metric === "download"
              ? sample.rxBytesPerSecond
              : sample.txBytesPerSecond;
      if (value === null || sample.at < end - 180_000) {
        previous = null;
        return [];
      }
      const x = Math.max(0, Math.min(300, ((sample.at - start) / duration) * 300));
      const y = 38 - Math.min(1, Math.max(0, value) / maximum) * 36;
      const command = previous === null || sample.at - previous > 10_000 ? "M" : "L";
      previous = sample.at;
      return [`${command}${x.toFixed(1)},${y.toFixed(1)}`];
    })
    .join(" ");
}

/** Both network series share one binary scale, based only on the visible history window. */
export function networkHistory(history: readonly HostSample[]) {
  const end = history.at(-1)?.at ?? 0;
  const peak = history.reduce(
    (maximum, sample) =>
      sample.at < end - 180_000
        ? maximum
        : Math.max(maximum, sample.rxBytesPerSecond ?? 0, sample.txBytesPerSecond ?? 0),
    1024,
  );
  const maximum = 2 ** Math.ceil(Math.log2(peak));
  return {
    maximum,
    download: historyPath(history, "download", maximum),
    upload: historyPath(history, "upload", maximum),
  };
}
