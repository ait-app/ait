import { describe, expect, it } from "vitest";
import type { HostSample } from "@ait/protocol/host-monitor";
import { bytes, historyPath, memoryPercent, percent, resolveMonitorHost } from "./model";

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
});
