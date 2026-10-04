import { z } from "zod";

export const RelayStatusSchema = z.object({
  serverId: z.string(),
  instanceId: z.string(),
  platform: z.string(),
  status: z.object({
    online: z.boolean(),
    connecting: z.boolean(),
    epoch: z.string().nullable(),
    error: z.string().nullable(),
  }),
});
export type RelayStatus = z.infer<typeof RelayStatusSchema>;

export const RelayControlGrantSchema = z.object({
  center_url: z.string(),
  control_ticket: z.string().regex(/^[a-fA-F0-9]{64}$/),
  node_session_id: z.string().uuid(),
});
export type RelayControlGrant = z.infer<typeof RelayControlGrantSchema>;

export const RelayStatusRequestSchema = z.object({
  type: z.literal("relay.status.request"),
  requestId: z.string(),
});
export const RelayStartRequestSchema = RelayControlGrantSchema.extend({
  type: z.literal("relay.start.request"),
  requestId: z.string(),
});
export const RelayStopRequestSchema = z.object({
  type: z.literal("relay.stop.request"),
  requestId: z.string(),
});
const payload = RelayStatusSchema.extend({ requestId: z.string() });
export const RelayStatusResponseSchema = z.object({
  type: z.literal("relay.status.response"),
  payload,
});
export const RelayStartResponseSchema = z.object({
  type: z.literal("relay.start.response"),
  payload,
});
export const RelayStopResponseSchema = z.object({
  type: z.literal("relay.stop.response"),
  payload,
});
