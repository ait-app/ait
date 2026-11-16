import { z } from "zod";

const bytes = z.number().nonnegative();
export const HostSampleSchema = z.object({
  at: z.number(),
  cpuPercent: z.number().min(0).max(100).nullable(),
  memoryUsedBytes: bytes,
  memoryTotalBytes: bytes,
  rxBytesPerSecond: bytes.nullable(),
  txBytesPerSecond: bytes.nullable(),
});

export const HostSnapshotSchema = z.object({
  platform: z.string(),
  cpuCount: z.number().int().nonnegative(),
  uptimeSeconds: bytes.nullable(),
  current: HostSampleSchema,
  history: z.array(HostSampleSchema).max(90),
  disks: z.array(z.object({ mount: z.string(), totalBytes: bytes, availableBytes: bytes })),
});

export const HostStatsRequestSchema = z.object({
  type: z.literal("daemon.host_stats.request"),
  requestId: z.string(),
  includeHistory: z.boolean().optional(),
});

export const HostStatsResponseSchema = z.object({
  type: z.literal("daemon.host_stats.response"),
  payload: HostSnapshotSchema.extend({ requestId: z.string() }),
});

export type HostSnapshot = z.infer<typeof HostSnapshotSchema>;
export type HostSample = z.infer<typeof HostSampleSchema>;
