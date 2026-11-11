import type { HostSample } from "@ait/protocol/host-monitor";

export interface MonitorHost {
  serverId: string;
  label: string;
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
export function historyPath(history: readonly HostSample[], metric: "cpu" | "memory"): string {
  const end = history.at(-1)?.at;
  if (end === undefined) return "";
  const start = Math.max(end - 180_000, history[0]?.at ?? end);
  const duration = Math.max(2_000, end - start);
  let previous: number | null = null;
  return history
    .flatMap((sample) => {
      const value = metric === "cpu" ? sample.cpuPercent : memoryPercent(sample);
      if (value === null || sample.at < end - 180_000) {
        previous = null;
        return [];
      }
      const x = Math.max(0, Math.min(300, ((sample.at - start) / duration) * 300));
      const y = 38 - Math.min(100, Math.max(0, value)) * 0.36;
      const command = previous === null || sample.at - previous > 10_000 ? "M" : "L";
      previous = sample.at;
      return [`${command}${x.toFixed(1)},${y.toFixed(1)}`];
    })
    .join(" ");
}
