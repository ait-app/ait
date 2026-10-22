import { z } from "zod";

export const HubStatusSchema = z.object({
  serverId: z.string(),
  instanceId: z.string(),
  platform: z.string(),
  management: z
    .object({
      mode: z.enum(["external", "managed"]),
      phase: z.string().optional(),
      binding: z
        .object({
          node_id: z.string().uuid(),
          host_id: z.string().uuid(),
          server_id: z.string().uuid(),
          grant_id: z.string().uuid(),
        })
        .nullable()
        .optional(),
    })
    .optional(),
  status: z.object({
    online: z.boolean(),
    connecting: z.boolean(),
    link: z.string().nullable(),
    error: z.string().nullable(),
  }),
});
export type HubStatus = z.infer<typeof HubStatusSchema>;

export const HubControlGrantSchema = z.object({
  center_url: z.string(),
  hub_ticket: z.string().regex(/^[a-fA-F0-9]{64}$/),
  node_session_id: z.string().uuid(),
});
export type HubControlGrant = z.infer<typeof HubControlGrantSchema>;

export const HubStatusRequestSchema = z.object({
  type: z.literal("hub.status.request"),
  requestId: z.string(),
});
export const HubStartRequestSchema = HubControlGrantSchema.extend({
  type: z.literal("hub.start.request"),
  requestId: z.string(),
});
export const HubStopRequestSchema = z.object({
  type: z.literal("hub.stop.request"),
  requestId: z.string(),
});
const payload = HubStatusSchema.extend({ requestId: z.string() });
export const HubStatusResponseSchema = z.object({
  type: z.literal("hub.status.response"),
  payload,
});
export const HubStartResponseSchema = z.object({
  type: z.literal("hub.start.response"),
  payload,
});
export const HubStopResponseSchema = z.object({
  type: z.literal("hub.stop.response"),
  payload,
});
