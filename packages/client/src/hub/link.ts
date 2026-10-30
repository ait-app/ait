import type { WebSocketFactory, WebSocketLike } from "../daemon-client-transport-types.js";
import { extractWebSocketMessage } from "../daemon-client-transport-utils.js";
import { formatDiagnosticFields } from "../diagnostic-log.js";
import { bindWsHandler, closeWebSocket } from "../daemon-client-websocket-transport.js";
import {
  decodeHubFrame,
  encodeHubFrame,
  HUB_PATH,
  type HubInboundHeader,
  type HubOutboundHeader,
  type HubWelcome,
} from "./frame.js";
import { HubSession, type HubSessionHeader, type HubSessionLink } from "./session.js";
import { HubCalls, HubCallError } from "./calls.js";
import type {
  HubAccountConnection,
  HubAccountSnapshot,
  HubAccountEvent,
  HubOrganization,
  HubSharedWorkspace,
} from "./account.js";

/** A one-use credential for `/v1/hub`, bound to the center that issued it. */
export interface HubTicket {
  centerUrl: string;
  ticket: string;
}

/** One serve endpoint of the account, as reported by `presence`. */
export interface HubPresenceNode {
  node_id: string;
  host_id: string | null;
  name: string;
  platform: string;
  app_version: string;
  state: "online" | "reconnecting";
}

/**
 * - `stopped`: not started, or stopped by sign-out/suspension.
 * - `connecting`: obtaining a ticket or completing the first handshake of a new link.
 * - `online`: welcomed on the current connection.
 * - `reconnecting`: the link is within its grace period and resuming; sessions stay open.
 * - `unavailable`: the last attempt failed and a retry is scheduled.
 */
export type HubLinkState = "stopped" | "connecting" | "online" | "reconnecting" | "unavailable";

export interface HubLinkLogger {
  info(fields: object, message: string): void;
  warn(fields: object, message: string): void;
}

export interface HubLinkOptions {
  /** Issue a fresh access ticket; rejections carrying `status: 404` mean the center has no hub. */
  ticket(): Promise<HubTicket>;
  webSocketFactory: WebSocketFactory;
  /** Native stacks send the credential as a Bearer header; browsers use a subprotocol. */
  authentication?: "bearer" | "subprotocol";
  randomUUID?: () => string;
  logger?: HubLinkLogger;
}

interface LinkIdentity {
  id: string;
  token: string | null;
  centerUrl: string;
  nodeId: string;
  graceMs: number;
  lostAt: number | null;
  methods: readonly string[];
}

const RECONNECT_BASE_MS = 300;
const RECONNECT_MAX_MS = 8000;
const FRESH_RETRY_MAX_MS = 30_000;
const UNAVAILABLE_RETRY_MS = 5 * 60_000;
const HANDSHAKE_TIMEOUT_MS = 10_000;
const ACK_DELAY_MS = 50;
const ACK_EVERY = 32;
const MAX_UNACKED_BYTES = 8 * 1024 * 1024;
/** Resume with a fresh ticket after this many failed resume-token connections. */
const TOKEN_ATTEMPTS = 2;
const DEFAULT_VERIFY_MS = 3000;
const HEARTBEAT_INTERVAL_MS = 20_000;
const SHARE_RETRY_MS = 3000;
const SHARE_SUBSCRIBE = "share.subscribe";

const consoleLogger: HubLinkLogger = {
  info: (fields, message) => console.info(message, formatDiagnosticFields(fields)),
  warn: (fields, message) => console.warn(message, formatDiagnosticFields(fields)),
};

function status(error: unknown): number | undefined {
  const value = (error as { status?: unknown } | null)?.status;
  return typeof value === "number" ? value : undefined;
}

function presenceNodes(payload: Uint8Array): HubPresenceNode[] {
  const value = JSON.parse(new TextDecoder().decode(payload)) as { nodes?: unknown };
  if (!Array.isArray(value?.nodes)) throw new Error("Invalid hub presence.");
  return value.nodes.filter(
    (node): node is HubPresenceNode =>
      !!node &&
      typeof node === "object" &&
      typeof node.node_id === "string" &&
      (node.host_id === null || typeof node.host_id === "string") &&
      (node.state === "online" || node.state === "reconnecting"),
  );
}

function snapshotItems(payload: Uint8Array, key: string): unknown[] {
  const value = JSON.parse(new TextDecoder().decode(payload)) as Record<string, unknown> | null;
  const items = value?.[key];
  if (!Array.isArray(items)) throw new Error(`Invalid hub ${key} snapshot.`);
  return items;
}

function strings(value: Record<string, unknown>, keys: readonly string[]): boolean {
  return keys.every((key) => typeof value[key] === "string");
}

function record(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

function organizations(payload: Uint8Array): HubOrganization[] {
  return snapshotItems(payload, "orgs").filter(
    (org): org is HubOrganization =>
      record(org) &&
      strings(org, ["organizationId", "name", "slug", "status", "myRole"]) &&
      typeof org.memberVersion === "number",
  );
}

function sharedWorkspaces(payload: Uint8Array): HubSharedWorkspace[] {
  return snapshotItems(payload, "shares").filter(
    (share): share is HubSharedWorkspace =>
      record(share) &&
      strings(share, [
        "shareId",
        "organizationId",
        "organizationName",
        "hostId",
        "nodeId",
        "hostName",
        "workspaceKey",
        "displayName",
        "permission",
      ]) &&
      (share.projectName === null || typeof share.projectName === "string") &&
      record(share.owner) &&
      strings(share.owner, ["userId", "displayName"]) &&
      (share.state === "online" || share.state === "reconnecting"),
  );
}

function same(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

/**
 * One persistent `ait-hub-v1` access link per process. Frames are numbered per direction and kept
 * until acknowledged; a dropped connection resumes the same link within the center's grace period
 * so open sessions continue without a new business hello or subscriptions.
 */
export class HubLink implements HubAccountConnection {
  private current: HubLinkState = "stopped";
  private socket: WebSocketLike | null = null;
  private detach: (() => void)[] = [];
  private generation = 0;
  private welcomed = false;
  private link: LinkIdentity | null = null;
  private sent = 0;
  private unacked: { seq: number; frame: Uint8Array }[] = [];
  private unackedBytes = 0;
  private recv = 0;
  private acked = 0;
  private failures = 0;
  private tokenFailures = 0;
  private useTicket = false;
  private nodes: HubPresenceNode[] | null = null;
  private orgSnapshot: HubOrganization[] | null = null;
  private shareSnapshot: HubSharedWorkspace[] | null = null;
  /** Active `subscribeShares()` interests. */
  private shareInterest = 0;
  /** Logical link that has accepted, or is processing, `share.subscribe`. */
  private shareLink: string | null = null;
  private nextPing = 0;
  private readonly pings = new Map<number, (ok: boolean) => void>();
  private verification: Promise<boolean> | null = null;
  private readonly sessions = new Map<string, HubSession>();
  private readonly listeners = new Set<() => void>();
  private readonly events = new Set<(event: HubAccountEvent) => void>();
  private readonly calls = new HubCalls();
  private retryTimer: ReturnType<typeof setTimeout> | null = null;
  private handshakeTimer: ReturnType<typeof setTimeout> | null = null;
  private ackTimer: ReturnType<typeof setTimeout> | null = null;
  private graceTimer: ReturnType<typeof setTimeout> | null = null;
  private heartbeatTimer: ReturnType<typeof setTimeout> | null = null;
  private shareTimer: ReturnType<typeof setTimeout> | null = null;
  private readonly logger: HubLinkLogger;
  private readonly sessionLink: HubSessionLink = {
    recoverLiveness: () => this.recoverLiveness(),
    send: (header, payload) => this.sendSequenced(header, payload),
    release: (id) => {
      this.sessions.delete(id);
    },
  };

  constructor(private readonly options: HubLinkOptions) {
    this.logger = options.logger ?? consoleLogger;
  }

  get state(): HubLinkState {
    return this.current;
  }

  /** Center of the current link, or null before the first welcome. */
  get centerUrl(): string | null {
    return this.link?.centerUrl ?? null;
  }

  snapshot(): HubAccountSnapshot {
    return {
      state: this.current,
      linkId: this.link?.id ?? null,
      centerUrl: this.centerUrl,
      methods: this.link?.methods ?? [],
    };
  }

  call<T = unknown>(method: string, params: unknown): Promise<T> {
    if (this.current !== "online")
      return Promise.reject(new HubCallError("offline", "Center is not connected."));
    if (!this.link?.methods.includes(method))
      return Promise.reject(new HubCallError("unsupported", "Center method is unavailable."));
    const id = this.options.randomUUID?.() ?? globalThis.crypto.randomUUID();
    let payload: Uint8Array;
    try {
      payload = new TextEncoder().encode(JSON.stringify(params));
    } catch {
      return Promise.reject(new HubCallError("invalid_params", "Invalid center parameters."));
    }
    return this.calls.request(id, payload, () =>
      this.sendSequenced({ t: "call", id, method }, payload),
    ) as Promise<T>;
  }

  onEvent(listener: (event: HubAccountEvent) => void): () => void {
    this.events.add(listener);
    return () => {
      this.events.delete(listener);
    };
  }

  /** Latest presence snapshot of the current link, or null before the first one. */
  get presence(): readonly HubPresenceNode[] | null {
    return this.nodes;
  }

  /** Online with a presence snapshot: new sessions can be routed by presence. */
  get ready(): boolean {
    return this.current === "online" && this.nodes !== null;
  }

  /** The serve endpoint publishing `hostId`, if presence lists one. */
  serveFor(hostId: string): HubPresenceNode | undefined {
    return this.nodes?.find((node) => node.host_id === hostId);
  }

  /** Organizations of the account from the latest `orgs` snapshot of this link, or null. */
  get orgs(): readonly HubOrganization[] | null {
    return this.orgSnapshot;
  }

  /** Workspaces shared with this account from the latest `share_presence`, or null. */
  get shares(): readonly HubSharedWorkspace[] | null {
    return this.shareSnapshot;
  }

  /** The current center accepts `share.subscribe` and guest sessions. */
  get sharesSupported(): boolean {
    return this.link?.methods.includes(SHARE_SUBSCRIBE) ?? false;
  }

  /**
   * Receive `orgs` and `share_presence` snapshots while the returned disposer is not called.
   * The subscription belongs to the logical link, so it is renewed after every fresh link.
   * Centers that do not advertise `share.subscribe` are not asked.
   */
  subscribeShares(): () => void {
    this.shareInterest += 1;
    this.syncShares();
    let active = true;
    return () => {
      if (!active) return;
      active = false;
      this.shareInterest -= 1;
      if (!this.shareInterest) this.clearTimer("share");
    };
  }

  private syncShares(): void {
    const link = this.link;
    if (
      !this.shareInterest ||
      this.current !== "online" ||
      !link ||
      this.shareLink === link.id ||
      this.shareTimer ||
      !link.methods.includes(SHARE_SUBSCRIBE)
    )
      return;
    this.shareLink = link.id;
    this.call(SHARE_SUBSCRIBE, {}).catch((error: unknown) => {
      // A reset link already forgot this attempt; its successor subscribes again.
      if (this.shareLink !== link.id) return;
      this.shareLink = null;
      this.logger.warn(
        { link: link.id, reason: error instanceof Error ? error.message : String(error) },
        "[Hub] share subscription failed",
      );
      this.shareTimer = setTimeout(() => {
        this.shareTimer = null;
        this.syncShares();
      }, SHARE_RETRY_MS);
    });
  }

  /** Observe state, presence, organization and share snapshot changes. */
  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  start(): void {
    if (this.current !== "stopped") return;
    this.setState("connecting");
    void this.connect();
  }

  /** Close the connection and end every session; the next start creates a new link. */
  stop(): void {
    if (this.current === "stopped") return;
    if (this.welcomed) {
      // Release the peer's resources now instead of after the grace period.
      for (const session of this.sessions.values()) {
        try {
          if (session.opened) this.sendSequenced({ t: "close", s: session.id, code: "closed" });
        } catch {
          /* A full send buffer cannot be flushed anyway; the center expires the link. */
        }
      }
    }
    this.generation += 1;
    this.closeSocket();
    this.clearTimer("retry");
    this.resetLink("stopped", false);
    this.setState("stopped");
  }

  /** A connection attempt is in progress, or the link is online awaiting its first presence. */
  get pending(): boolean {
    return (
      !this.ready &&
      (this.current === "connecting" ||
        this.current === "reconnecting" ||
        this.current === "online")
    );
  }

  /** Resolve true once the link is ready; false at the timeout or when no attempt is pending. */
  whenReady(timeoutMs: number): Promise<boolean> {
    if (this.ready) return Promise.resolve(true);
    if (!this.pending) return Promise.resolve(false);
    return new Promise((resolve) => {
      const done = (value: boolean) => {
        clearTimeout(timer);
        remove();
        resolve(value);
      };
      const timer = setTimeout(() => done(false), timeoutMs);
      const remove = this.subscribe(() => {
        if (this.ready) done(true);
        else if (!this.pending) done(false);
      });
    });
  }

  /**
   * Prove the connection is alive, e.g. after returning to the foreground. A reconnect waiting for
   * its backoff starts at once; an unanswered ping forces a reconnect.
   */
  verify(timeoutMs = DEFAULT_VERIFY_MS): Promise<boolean> {
    if (this.current === "stopped") return Promise.resolve(false);
    if (this.verification) return this.verification;
    if (!this.welcomed || !this.socket) {
      if (this.retryTimer) {
        this.clearTimer("retry");
        void this.connect();
      }
      return Promise.resolve(false);
    }
    const socket = this.socket;
    const generation = this.generation;
    const n = ++this.nextPing;
    this.clearTimer("heartbeat");
    const verification = new Promise<boolean>((resolve) => {
      const timer = setTimeout(() => {
        this.pings.delete(n);
        resolve(false);
        this.drop(generation, "verify_timeout");
      }, timeoutMs);
      this.pings.set(n, (ok) => {
        clearTimeout(timer);
        this.pings.delete(n);
        resolve(ok);
      });
      try {
        socket.send(encodeHubFrame({ t: "ping", n }));
      } catch {
        this.drop(generation, "send_failed");
      }
    });
    this.verification = verification;
    void verification.then((ok) => {
      if (this.verification === verification) this.verification = null;
      if (ok && generation === this.generation) this.scheduleHeartbeat();
    });
    return verification;
  }

  /** Recover only the current link; a new link cannot preserve its sessions. */
  private async recoverLiveness(): Promise<boolean> {
    const link = this.link;
    if (!link || (await this.verify()) || this.link !== link) return false;
    if (this.welcomed) return true;
    // drop() has armed the grace timer. Its expiry, stop(), or a rejection resets
    // the identity and notifies this waiter, so recovery cannot wait indefinitely.
    return new Promise((resolve) => {
      const remove = this.subscribe(() => {
        if (this.link !== link || this.welcomed) {
          remove();
          resolve(this.link === link && this.welcomed);
        }
      });
    });
  }

  private scheduleHeartbeat(): void {
    this.clearTimer("heartbeat");
    if (!this.welcomed) return;
    this.heartbeatTimer = setTimeout(() => {
      this.heartbeatTimer = null;
      void this.verify();
    }, HEARTBEAT_INTERVAL_MS);
  }

  /**
   * Create a session to `nodeId`'s serve endpoint; its first send opens it. With `share`, the
   * session is a guest session to a workspace another account shares (see `shares`).
   */
  openSession(nodeId: string, options: { share?: string } = {}): HubSession {
    if (this.current === "stopped") throw new Error("Hub link is stopped.");
    // Older centers reject unknown header fields and would drop the whole link.
    if (options.share !== undefined && !this.sharesSupported)
      throw new HubCallError("unsupported", "Center does not support shared workspaces.");
    if (this.sessions.size >= 16) throw new Error("Too many hub sessions.");
    const id = (this.options.randomUUID ?? (() => crypto.randomUUID()))();
    const session = new HubSession(this.sessionLink, id, nodeId, options.share);
    this.sessions.set(id, session);
    return session;
  }

  private setState(next: HubLinkState): void {
    if (this.current === next) return;
    this.current = next;
    this.emit();
  }

  private emit(): void {
    this.syncShares();
    for (const listener of this.listeners) listener();
  }

  private clearTimer(name: "retry" | "handshake" | "ack" | "grace" | "heartbeat" | "share"): void {
    const key = `${name}Timer` as const;
    const timer = this[key];
    if (timer) clearTimeout(timer);
    this[key] = null;
  }

  private async connect(): Promise<void> {
    this.clearTimer("retry");
    const generation = ++this.generation;
    if (this.link?.lostAt != null && Date.now() - this.link.lostAt >= this.link.graceMs)
      this.resetLink("grace_expired");
    const link = this.link;
    this.setState(link ? "reconnecting" : "connecting");
    let centerUrl: string;
    let credential: string;
    if (link?.token && !this.useTicket) {
      centerUrl = link.centerUrl;
      credential = link.token;
    } else {
      try {
        const ticket = await this.options.ticket();
        if (generation !== this.generation) return;
        if (!/^[0-9a-f]{64}$/i.test(ticket.ticket)) throw new Error("Invalid hub ticket.");
        // A ticket from another center can never resume this link.
        if (this.link && ticket.centerUrl !== this.link.centerUrl) this.resetLink("center_changed");
        centerUrl = ticket.centerUrl;
        credential = ticket.ticket;
      } catch (error) {
        if (generation === this.generation) this.retry(error);
        return;
      }
    }
    const url = new URL(`${centerUrl}${HUB_PATH}`);
    url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
    let socket: WebSocketLike;
    try {
      socket = this.options.webSocketFactory(
        url.toString(),
        this.options.authentication === "subprotocol"
          ? { protocols: [`ait.ticket.${credential}`] }
          : { headers: { Authorization: `Bearer ${credential}` } },
      );
    } catch (error) {
      this.retry(error);
      return;
    }
    try {
      socket.binaryType = "arraybuffer";
    } catch {
      /* Some runtimes expose a read-only binaryType and already deliver ArrayBuffers. */
    }
    this.socket = socket;
    this.welcomed = false;
    this.acked = this.recv;
    this.detach = [
      bindWsHandler(socket, "open", () => {
        if (generation !== this.generation) return;
        const resume = this.link ? { link: this.link.id, recv: this.recv } : null;
        this.write(generation, encodeHubFrame({ t: "hello", v: 1, resume }));
      }),
      bindWsHandler(socket, "message", (...args) => {
        if (generation !== this.generation) return;
        const message = extractWebSocketMessage(
          args[0],
          typeof args[1] === "boolean" ? args[1] : undefined,
        );
        this.receive(generation, message.data, message.isBinary, centerUrl);
      }),
      bindWsHandler(socket, "close", () => this.drop(generation, "closed")),
      bindWsHandler(socket, "error", () => this.drop(generation, "error")),
    ];
    this.handshakeTimer = setTimeout(
      () => this.drop(generation, "handshake_timeout"),
      HANDSHAKE_TIMEOUT_MS,
    );
  }

  private write(generation: number, frame: Uint8Array): void {
    try {
      this.socket?.send(frame);
    } catch {
      this.drop(generation, "send_failed");
    }
  }

  private receive(
    generation: number,
    data: string | ArrayBuffer,
    binary: boolean,
    centerUrl: string,
  ): void {
    let header: HubInboundHeader;
    let payload: Uint8Array;
    try {
      if (!binary || typeof data === "string") throw new Error("Hub frames must be binary.");
      ({ header, payload } = decodeHubFrame(data));
    } catch {
      this.drop(generation, "invalid_frame");
      return;
    }
    if (header.t === "error") {
      this.serverError(header.code);
      return;
    }
    if (!this.welcomed) {
      if (header.t === "welcome") this.welcome(generation, header, centerUrl);
      else this.drop(generation, "unexpected_frame");
      return;
    }
    switch (header.t) {
      case "welcome":
        this.drop(generation, "unexpected_frame");
        return;
      case "ack":
        this.release(header.a);
        return;
      case "pong":
        this.pings.get(header.n)?.(true);
        return;
      case "open":
      case "accept":
      case "msg":
      case "close":
      case "presence":
      case "orgs":
      case "share_presence":
      case "reply":
      case "event":
        break;
      default:
        return;
    }
    this.scheduleAck(generation);
    // Duplicates can follow a resume; frames already delivered are dropped.
    if (header.seq <= this.recv) return;
    this.recv = header.seq;
    if (header.t === "reply") {
      this.calls.reply(header.id, payload);
      return;
    }
    if (header.t === "event") {
      try {
        const event: HubAccountEvent = {
          subscriptionId: header.sub,
          cursor: header.cursor,
          hostId: header.host_id,
          eventId: header.event_id,
          resourceId: header.resource_id,
          method: header.method,
          params: JSON.parse(new TextDecoder().decode(payload)),
        };
        for (const listener of this.events) listener(event);
      } catch {
        // A malformed or unconsumed event must be recovered using the business cursor.
        this.drop(generation, "invalid_event");
      }
      return;
    }
    if (header.t === "presence") {
      try {
        this.nodes = presenceNodes(payload);
      } catch {
        this.logger.warn({ link: this.link?.id }, "[Hub] invalid presence ignored");
        return;
      }
      this.emit();
      return;
    }
    if (header.t === "orgs" || header.t === "share_presence") {
      this.snapshotReceived(header.t, payload);
      return;
    }
    // Broadcasts carry no session; the access side does not consume them yet.
    if (header.t === "open" || (header.t === "msg" && !header.s)) return;
    const id = header.s;
    if (id) this.sessions.get(id)?.receive(header, payload);
  }

  /** Newer snapshots replace older ones; malformed ones are ignored like presence. */
  private snapshotReceived(kind: "orgs" | "share_presence", payload: Uint8Array): void {
    let changed: boolean;
    try {
      if (kind === "orgs") {
        const next = organizations(payload);
        changed = !same(next, this.orgSnapshot);
        if (changed) this.orgSnapshot = next;
      } else {
        const next = sharedWorkspaces(payload);
        changed = !same(next, this.shareSnapshot);
        if (changed) this.shareSnapshot = next;
      }
    } catch {
      this.logger.warn({ link: this.link?.id, kind }, "[Hub] invalid snapshot ignored");
      return;
    }
    if (changed) this.emit();
  }

  private welcome(generation: number, welcome: HubWelcome, centerUrl: string): void {
    this.clearTimer("handshake");
    if (!welcome.resumed) {
      // A new link replaces the old one: its sessions and sequence numbers are gone.
      if (this.link) this.resetLink("link_reset");
      if (!welcome.resume_token) {
        this.drop(generation, "invalid_welcome");
        return;
      }
      this.link = {
        id: welcome.link,
        token: welcome.resume_token,
        centerUrl,
        nodeId: welcome.node_id,
        graceMs: welcome.grace_seconds * 1000,
        lostAt: null,
        methods: welcome.center_methods ?? [],
      };
    } else if (!this.link || this.link.id !== welcome.link || welcome.recv > this.sent) {
      this.serverError("invalid_resume");
      return;
    } else {
      this.link.lostAt = null;
      this.link.methods = welcome.center_methods ?? [];
    }
    this.welcomed = true;
    this.failures = 0;
    this.tokenFailures = 0;
    this.useTicket = false;
    this.clearTimer("grace");
    this.release(welcome.recv);
    for (const item of this.unacked) this.write(generation, item.frame);
    if (generation !== this.generation) return;
    this.logger.info(
      { link: welcome.link, resumed: welcome.resumed, resent: this.unacked.length },
      "[Hub] link online",
    );
    this.setState("online");
    this.scheduleHeartbeat();
  }

  /** Drop frames the center has acknowledged. */
  private release(acknowledged: number): void {
    while (this.unacked.length && this.unacked[0].seq <= acknowledged) {
      this.unackedBytes -= this.unacked.shift()!.frame.byteLength;
    }
  }

  private scheduleAck(generation: number): void {
    // Coalesce acknowledgements: one per short burst, or sooner under sustained traffic.
    if (this.recv - this.acked >= ACK_EVERY) {
      this.flushAck(generation);
      return;
    }
    this.ackTimer ??= setTimeout(() => {
      this.ackTimer = null;
      this.flushAck(generation);
    }, ACK_DELAY_MS);
  }

  private flushAck(generation: number): void {
    if (generation !== this.generation || !this.welcomed || this.recv <= this.acked) return;
    this.acked = this.recv;
    this.write(generation, encodeHubFrame({ t: "ack", a: this.recv }));
  }

  private sendSequenced(
    header: HubSessionHeader | { t: "call"; id: string; method: string },
    payload?: Uint8Array,
  ): void {
    if (this.current === "stopped") throw new Error("Hub link is stopped.");
    const length = payload?.byteLength ?? 0;
    if (this.unackedBytes + length > MAX_UNACKED_BYTES) throw new Error("Hub send buffer is full.");
    const seq = this.sent + 1;
    const frame = encodeHubFrame({ ...header, seq } as HubOutboundHeader, payload);
    if (this.unackedBytes + frame.byteLength > MAX_UNACKED_BYTES)
      throw new Error("Hub send buffer is full.");
    this.sent = seq;
    this.unacked.push({ seq, frame });
    this.unackedBytes += frame.byteLength;
    // While reconnecting, frames wait in the buffer and are resent after the welcome.
    if (this.welcomed) this.write(this.generation, frame);
  }

  private closeSocket(): void {
    const socket = this.socket;
    this.socket = null;
    this.welcomed = false;
    for (const remove of this.detach) remove();
    this.detach = [];
    this.clearTimer("handshake");
    this.clearTimer("ack");
    this.clearTimer("heartbeat");
    this.verification = null;
    for (const resolve of this.pings.values()) resolve(false);
    try {
      if (socket) closeWebSocket(socket);
    } catch {
      /* Already closed. */
    }
  }

  /** The physical connection ended; resume within grace, otherwise start a new link. */
  private drop(generation: number, reason: string): void {
    if (generation !== this.generation || this.current === "stopped") return;
    const welcomed = this.welcomed;
    this.generation += 1;
    this.closeSocket();
    const link = this.link;
    if (link) {
      if (!welcomed && link.token && !this.useTicket && ++this.tokenFailures >= TOKEN_ATTEMPTS)
        this.useTicket = true;
      if (link.lostAt === null) {
        link.lostAt = Date.now();
        this.graceTimer = setTimeout(() => {
          this.graceTimer = null;
          if (this.link === link && link.lostAt !== null) {
            this.resetLink("grace_expired");
            if (this.current === "reconnecting") this.setState("connecting");
          }
        }, link.graceMs);
      }
    }
    this.logger.info({ link: link?.id, reason, welcomed }, "[Hub] connection lost");
    // The first reconnect after a live connection is immediate; later ones back off.
    this.retry(reason, welcomed ? 0 : undefined);
  }

  /** Error frames end the link on the center: reconnect with a fresh ticket. */
  private serverError(code: string): void {
    this.logger.warn({ link: this.link?.id, code }, "[Hub] center rejected the link");
    this.generation += 1;
    this.closeSocket();
    this.resetLink(code);
    const resumeFailure = code === "resume_expired" || code === "invalid_resume";
    this.retry(code, resumeFailure ? 0 : undefined);
  }

  private retry(reason: unknown, delay?: number): void {
    this.clearTimer("retry");
    if (this.current === "stopped") return;
    const unavailable = status(reason) === 404;
    const wait =
      delay ??
      (unavailable
        ? UNAVAILABLE_RETRY_MS
        : Math.min(
            this.link ? RECONNECT_MAX_MS : FRESH_RETRY_MAX_MS,
            RECONNECT_BASE_MS * 2 ** Math.min(this.failures++, 10),
          ) *
          (0.8 + Math.random() * 0.4));
    if (delay === undefined)
      this.logger.warn(
        {
          link: this.link?.id,
          reason: reason instanceof Error ? reason.message : String(reason),
          retryMs: Math.round(wait),
        },
        "[Hub] connection attempt failed",
      );
    this.setState(this.link ? "reconnecting" : delay === 0 ? "connecting" : "unavailable");
    this.retryTimer = setTimeout(() => void this.connect(), wait);
  }

  /** Forget the link: fail its sessions and restart sequence numbers. */
  private resetLink(reason: string, notify = true): void {
    const sessions = [...this.sessions.values()];
    this.sessions.clear();
    this.link = null;
    this.sent = this.recv = this.acked = this.unackedBytes = 0;
    this.unacked = [];
    this.nodes = null;
    this.orgSnapshot = null;
    this.shareSnapshot = null;
    this.shareLink = null;
    this.clearTimer("share");
    this.useTicket = false;
    this.tokenFailures = 0;
    this.clearTimer("grace");
    const error = new Error(`Hub link reset (${reason}).`);
    this.calls.reset(reason);
    for (const session of sessions) session.fail(error);
    if (notify) this.emit();
  }
}
