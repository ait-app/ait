import { z } from "zod";
import { RelayStatusSchema } from "./relay.js";

/** Write-only authorization; the daemon persists it before consuming the one-use code. */
export const DeviceAuthorizationSchema = z.object({
  kind: z.literal("device_code"),
  device_code: z.string().regex(/^[a-fA-F0-9]{64}$/),
  user_code: z.string().regex(/^[a-fA-F0-9]{4}-[a-fA-F0-9]{4}-[a-fA-F0-9]{4}$/),
  request_id: z.string().uuid(),
  expires_at: z.string().datetime({ offset: true }),
  interval: z.number().int().min(5).max(300),
});
export type DeviceAuthorization = z.infer<typeof DeviceAuthorizationSchema>;

export const PublicationAuthorizationSchema = z.discriminatedUnion("kind", [
  DeviceAuthorizationSchema,
  z.object({
    kind: z.literal("enrollment"),
    token: z.string().min(1).max(8192),
    request_id: z.string().uuid(),
  }),
  z.object({
    kind: z.literal("credential"),
    request_id: z.string().uuid(),
    refresh_token: z.string().min(1).max(8192),
    refresh_expires_at: z.string().datetime({ offset: true }),
    binding: z.object({
      node_id: z.string().uuid(),
      host_id: z.string().uuid(),
      server_id: z.string().uuid(),
      grant_id: z.string().uuid(),
    }),
  }),
]);
export type PublicationAuthorization = z.infer<typeof PublicationAuthorizationSchema>;

export const PublicationStatusSchema = RelayStatusSchema.omit({ status: true }).extend({
  hub: z.object({
    status: z.object({
      online: z.boolean(),
      connecting: z.boolean(),
      error: z.string().nullable(),
      rejected: z.string().nullable(),
    }),
  }),
  relay: RelayStatusSchema.shape.status,
});
export type PublicationStatus = z.infer<typeof PublicationStatusSchema>;

export const PublicationStatusRequestSchema = z.object({
  type: z.literal("publication.status.request"),
  requestId: z.string(),
});
export const PublicationAuthorizeRequestSchema = z.object({
  type: z.literal("publication.authorize.request"),
  requestId: z.string(),
  authorization: PublicationAuthorizationSchema,
  mode: z.enum(["explicit", "initial"]).optional(),
});
export const PublicationStopRequestSchema = z.object({
  type: z.literal("publication.stop.request"),
  requestId: z.string(),
});
const payload = PublicationStatusSchema.extend({ requestId: z.string() });
export const PublicationStatusResponseSchema = z.object({
  type: z.literal("publication.status.response"),
  payload,
});
export const PublicationAuthorizeResponseSchema = z.object({
  type: z.literal("publication.authorize.response"),
  payload,
});
export const PublicationStopResponseSchema = z.object({
  type: z.literal("publication.stop.response"),
  payload,
});
