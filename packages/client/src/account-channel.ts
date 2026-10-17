import { z } from "zod";
import type { DaemonTransport } from "./daemon-client-transport-types.js";

const hostSchema = z.object({
  host_id: z.string(),
  node_id: z.string().nullable(),
  server_id: z.string().nullable(),
  instance_id: z.string().nullable(),
  name: z.string(),
  hostname: z.string().nullable().optional(),
  platform: z.string().nullable(),
  app_version: z.string().nullable().optional(),
  relay_modes: z.array(z.string()),
  online: z.boolean(),
  status: z.string(),
  last_seen_at: z.string().nullable().optional(),
  offline_since: z.string().nullable().optional(),
});

export type ChannelHost = z.infer<typeof hostSchema>;

const welcomeSchema = z.object({
  protocol: z.literal("ait-channel-v1"),
  connection_id: z.string(),
  heartbeat_interval_seconds: z.number().int().positive().max(300),
  heartbeat_timeout_seconds: z.number().int().positive().max(600),
});
const snapshotSchema = z.object({ hosts: z.array(hostSchema), observed_at: z.string() });
const nodeSchema = z.object({
  node_id: z.string(),
  node_session_id: z.string(),
  host_id: z.string(),
  observed_at: z.string(),
});

const onlineSchema = nodeSchema
  .extend({
    host: hostSchema.extend({
      online: z.literal(true),
      node_id: z.string().min(1),
      server_id: z.string().min(1),
      instance_id: z.string().min(1),
      relay_modes: z.array(z.string()).min(1),
    }),
  })
  .refine((event) => event.host.host_id === event.host_id && event.host.node_id === event.node_id);

/** One ticket-authenticated status connection; the account manager owns reconnects and leases. */
export class AccountChannel {
  private disposed = false;
  private welcomed = false;
  private deadline = Date.now() + 10_000;
  private heartbeatAt = Infinity;
  private interval = 20_000;
  private timeout = 60_000;
  private remove: (() => void)[] = [];
  private timer: ReturnType<typeof setInterval> | null = null;
  private verification: ReturnType<typeof setTimeout> | null = null;

  constructor(
    private readonly transport: DaemonTransport,
    private readonly handlers: {
      snapshot(hosts: ChannelHost[]): void;
      online(host: ChannelHost): void;
      offline(hostId: string, observedAt: string): void;
      failed(recoverImmediately: boolean): void;
    },
  ) {}

  start(): void {
    this.remove = [
      this.transport.onMessage((data, binary) => this.receive(data, binary)),
      this.transport.onClose(() => this.fail()),
      this.transport.onError(() => this.fail()),
    ];
    this.timer = setInterval(() => {
      if (Date.now() >= this.deadline) this.fail();
      else if (Date.now() >= this.heartbeatAt) this.refresh();
    }, 1000);
    this.timer.unref?.();
  }

  /** Heartbeats request a cached snapshot; observed_at is never a liveness clock. */
  refresh(options?: { verify?: boolean }): void {
    if (this.disposed) return;
    if (options?.verify) {
      if (this.verification) return;
      // Foreground recovery must not wait for the ordinary 60-second heartbeat
      // timeout. Arm before sending: native adapters can reply synchronously.
      this.verification = setTimeout(() => this.fail(), 3_000);
      this.verification.unref?.();
    }
    if (Date.now() >= this.deadline) {
      this.fail();
      return;
    }
    if (!this.welcomed) return;
    try {
      this.transport.send(JSON.stringify({ type: "channel.heartbeat" }));
      this.heartbeatAt = Date.now() + this.interval;
    } catch {
      this.fail();
    }
  }

  close(): void {
    if (this.disposed) return;
    this.disposed = true;
    if (this.timer) clearInterval(this.timer);
    this.clearVerification();
    for (const remove of this.remove) remove();
    this.remove = [];
    this.transport.close();
  }

  private fail(): void {
    if (this.disposed) return;
    const recoverImmediately = this.verification !== null;
    this.close();
    this.handlers.failed(recoverImmediately);
  }

  private clearVerification(): void {
    if (this.verification) clearTimeout(this.verification);
    this.verification = null;
  }

  private receive(data: unknown, binary: boolean): void {
    if (this.disposed) return;
    try {
      if (binary || typeof data !== "string") throw new Error("Invalid channel frame");
      const message: unknown = JSON.parse(data);
      const { type } = z.object({ type: z.string() }).parse(message);
      if (type === "channel.error" || type === "control.error") {
        this.fail();
        return;
      }
      if (type === "channel.welcome") {
        if (this.welcomed) throw new Error("Repeated channel welcome");
        const welcome = welcomeSchema.parse(message);
        this.interval = welcome.heartbeat_interval_seconds * 1000;
        this.timeout = welcome.heartbeat_timeout_seconds * 1000;
        if (this.interval >= this.timeout) throw new Error("Invalid heartbeat interval");
        this.welcomed = true;
        this.heartbeatAt = Date.now() + this.interval;
      } else if (type === "hosts.snapshot") {
        if (!this.welcomed) throw new Error("Missing channel welcome");
        this.handlers.snapshot(snapshotSchema.parse(message).hosts);
      } else if (type === "node.online") {
        if (!this.welcomed) throw new Error("Missing channel welcome");
        this.handlers.online(onlineSchema.parse(message).host);
      } else if (type === "node.offline") {
        if (!this.welcomed) throw new Error("Missing channel welcome");
        const event = nodeSchema.parse(message);
        this.handlers.offline(event.host_id, event.observed_at);
      }
      // Unknown server event types are deliberately ignored for forward compatibility.
      if (this.welcomed) {
        this.clearVerification();
        this.deadline = Date.now() + this.timeout;
      }
    } catch {
      this.fail();
    }
  }
}
