import { expect, it } from "vitest";
import { SessionInboundMessageSchema, WSOutboundMessageSchema } from "./messages.js";
import { HostSnapshotSchema } from "./host-monitor.js";

it("validates host requests and keeps unavailable counters distinct from zero", () => {
  expect(
    SessionInboundMessageSchema.safeParse({
      type: "daemon.host_stats.request",
      requestId: "host",
      includeHistory: true,
    }).success,
  ).toBe(true);
  const snapshot = {
    platform: "linux",
    cpuCount: 8,
    uptimeSeconds: 100,
    current: {
      at: 123,
      cpuPercent: null,
      memoryUsedBytes: 100,
      memoryTotalBytes: 200,
      rxBytesPerSecond: null,
      txBytesPerSecond: 0,
    },
    history: [],
    disks: [],
  };
  expect(HostSnapshotSchema.parse(snapshot).current.txBytesPerSecond).toBe(0);
  expect(
    WSOutboundMessageSchema.safeParse({
      type: "session",
      message: { type: "daemon.host_stats.response", payload: { ...snapshot, requestId: "host" } },
    }).success,
  ).toBe(true);
  expect(
    HostSnapshotSchema.safeParse({ ...snapshot, history: Array(91).fill(snapshot.current) })
      .success,
  ).toBe(false);
});
