import { describe, expect, it } from "vitest";
import type { HostSample } from "@ait/protocol/host-monitor";
import {
  bytes,
  historyPath,
  memoryPercent,
  networkHistory,
  percent,
  resolveMonitorHost,
} from "./model";

const sample: HostSample = {
  at: 180000,
  cpuPercent: 25,
  memoryUsedBytes: 512,
  memoryTotalBytes: 1024,
  rxBytesPerSecond: null,
  txBytesPerSecond: null,
};
describe("host monitor presentation", () => {
  it("follows the workspace but preserves explicitly chosen and offline hosts", () => {
    const hosts = [
      { serverId: "local", label: "Local" },
      { serverId: "remote", label: "Remote" },
    ];
    expect(resolveMonitorHost(hosts, null, "remote")).toBe(hosts[1]);
    expect(resolveMonitorHost(hosts, "local", "remote")).toBe(hosts[0]);
    expect(resolveMonitorHost(hosts, "removed", "remote")).toBe(hosts[1]);
    expect(resolveMonitorHost([], null, null)).toBeNull();
  });
  it("distinguishes missing counters from zero, bounds percentages and uses binary units", () => {
    expect(percent(null)).toBe("—");
    expect(percent(0)).toBe("0%");
    expect(percent(0.37)).toBe("0.4%");
    expect(percent(110)).toBe("100%");
    expect(bytes(1024 ** 3)).toBe("1 GiB");
    expect(bytes(null)).toBe("—");
    expect(memoryPercent(sample)).toBe(50);
    expect(memoryPercent({ ...sample, memoryTotalBytes: 0 })).toBeNull();
  });
  it("plots real sample times and breaks charts across sampling pauses", () => {
    expect(historyPath([], "cpu")).toBe("");
    const history = [sample, { ...sample, at: 182000 }, { ...sample, at: 210000 }];
    const path = historyPath(history, "cpu");
    expect(path.match(/M/g)).toHaveLength(2);
    expect(path.match(/L/g)).toHaveLength(1);
    expect(path).toContain("M300.0,29.0");
    expect(historyPath([{ ...sample, cpuPercent: null }], "cpu")).toBe("");
  });
  it("compares upload and download on one scale and excludes expired peaks", () => {
    const history = [
      { ...sample, at: 0, rxBytesPerSecond: 1024 ** 3 },
      { ...sample, at: 200000, rxBytesPerSecond: 4096, txBytesPerSecond: 1024 },
      { ...sample, at: 202000, rxBytesPerSecond: 4096, txBytesPerSecond: 1024 },
    ];
    const chart = networkHistory(history);
    expect(chart.maximum).toBe(4096);
    expect(chart.download).toContain("L300.0,2.0");
    expect(chart.upload).toContain("L300.0,29.0");
  });
  it("keeps missing network samples and pauses distinct from real zero traffic", () => {
    expect(networkHistory([sample])).toEqual({ maximum: 1024, download: "", upload: "" });
    const history = [
      { ...sample, rxBytesPerSecond: 0, txBytesPerSecond: 0 },
      { ...sample, at: 182000, rxBytesPerSecond: 0, txBytesPerSecond: null },
      { ...sample, at: 184000, rxBytesPerSecond: 0, txBytesPerSecond: 0 },
      { ...sample, at: 200000, rxBytesPerSecond: 0, txBytesPerSecond: 0 },
    ];
    const chart = networkHistory(history);
    expect(chart.download.match(/L/g)).toHaveLength(2);
    expect(chart.download).toContain("M300.0,38.0");
    expect(chart.upload).not.toContain("L");
  });
});
