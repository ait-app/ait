import { vi } from "vitest";
import type { WebSocketLike } from "../src/daemon-client-transport-types.js";
import { HUB_CENTER_FEATURES } from "../src/hub/frame.js";
import { HubLink, type HubLinkOptions, type HubTicket } from "../src/hub/link.js";

export const CENTER = "https://center.test/api";
/** Hub fields of a current center's node registration: its features and the link's first ticket. */
export const REGISTRATION_HUB = {
  hub_features: [...HUB_CENTER_FEATURES],
  hub_ticket: "e".repeat(64),
  hub_path: "/v1/hub",
  hub_expires_in_seconds: 30,
  hub_ep: "access",
};
export const RESUME_TOKEN = "r".repeat(64);

interface Sent {
  header: Record<string, unknown>;
  payload: Uint8Array;
}

/** A WebSocket double speaking raw hub frames; tests play the center. */
export class FakeHubSocket implements WebSocketLike {
  readyState = 0;
  binaryType = "blob";
  readonly sent: Sent[] = [];
  /** Make the next sends throw, as a socket whose buffer is gone would. */
  failSends = false;
  readonly close = vi.fn(() => {
    this.readyState = 3;
  });
  private readonly listeners = new Map<string, Set<(event: unknown) => void>>();

  constructor(
    readonly url: string,
    readonly options?: { headers?: Record<string, string>; protocols?: string[] },
  ) {}

  addEventListener(event: string, listener: (event: unknown) => void): void {
    if (!this.listeners.has(event)) this.listeners.set(event, new Set());
    this.listeners.get(event)!.add(listener);
  }

  removeEventListener(event: string, listener: (event: unknown) => void): void {
    this.listeners.get(event)?.delete(listener);
  }

  send(data: string | Uint8Array | ArrayBuffer): void {
    if (this.failSends) throw new Error("Socket is closed");
    if (typeof data === "string") throw new Error("Hub frames must be binary");
    const bytes = data instanceof Uint8Array ? data : new Uint8Array(data);
    const length = (bytes[0] << 8) | bytes[1];
    this.sent.push({
      header: JSON.parse(new TextDecoder().decode(bytes.subarray(2, 2 + length))),
      payload: bytes.slice(2 + length),
    });
  }

  /** Headers this client sent, optionally of one kind. */
  frames(t?: string): Record<string, unknown>[] {
    return this.sent.map((frame) => frame.header).filter((header) => !t || header.t === t);
  }

  open(): void {
    this.readyState = 1;
    this.emit("open", {});
  }

  receive(header: object, payload: Uint8Array | string = new Uint8Array()): void {
    const json = new TextEncoder().encode(JSON.stringify(header));
    const body = typeof payload === "string" ? new TextEncoder().encode(payload) : payload;
    const frame = new Uint8Array(2 + json.byteLength + body.byteLength);
    frame.set([json.byteLength >> 8, json.byteLength & 0xff]);
    frame.set(json, 2);
    frame.set(body, 2 + json.byteLength);
    this.emit("message", { data: frame.buffer });
  }

  /** A welcome enabling every feature of this connection's hello unless `overrides.f` says otherwise. */
  welcome(overrides: Record<string, unknown> = {}): void {
    this.receive({
      t: "welcome",
      v: 1,
      link: "link-1",
      resumed: false,
      recv: 0,
      resume_token: RESUME_TOKEN,
      node_id: "phone",
      grace_seconds: 120,
      ping_seconds: 20,
      max_payload: 1024 * 1024,
      f: this.frames("hello").at(-1)?.f,
      ...overrides,
    });
  }

  /** A full `hosts` snapshot; its version defaults to `seq`. */
  hosts(seq: number, hosts: object[], ver = seq): void {
    this.receive({ t: "hosts", seq, ver, full: true }, JSON.stringify({ hosts }));
  }

  drop(): void {
    this.readyState = 3;
    this.emit("close", { code: 1006 });
  }

  /** A transport error, as a failed upgrade or reset connection reports it. */
  fail(): void {
    this.emit("error", new Error("socket error"));
  }

  /** A text message, which the hub protocol never sends. */
  receiveText(text: string): void {
    this.emit("message", { data: text });
  }

  private emit(event: string, value: unknown): void {
    for (const listener of this.listeners.get(event) ?? []) listener(value);
  }
}

export const HOST = {
  node_id: "host-node",
  host_id: "11111111-1111-4111-8111-111111111111",
  name: "Workstation",
  platform: "linux",
  app_version: "test",
  state: "online",
};

/** A link with a scripted ticket authority and fake sockets. Uses fake timers. */
export function hubFixture(options: Partial<HubLinkOptions> = {}) {
  vi.useFakeTimers();
  vi.spyOn(Math, "random").mockReturnValue(0.5);
  const sockets: FakeHubSocket[] = [];
  let issued = 0;
  let sessions = 0;
  const ticket = vi.fn(async () => ({
    centerUrl: CENTER,
    ticket: (++issued).toString(16).padStart(64, "0"),
  }));
  const factory = vi.fn(
    (url: string, socketOptions?: { headers?: Record<string, string>; protocols?: string[] }) => {
      const socket = new FakeHubSocket(url, socketOptions);
      sockets.push(socket);
      return socket;
    },
  );
  const logger = { info: vi.fn(), warn: vi.fn() };
  const link = new HubLink({
    ticket,
    webSocketFactory: factory,
    randomUUID: () => `session-${++sessions}`,
    logger,
    ...options,
  });
  const flush = () => vi.advanceTimersByTimeAsync(0);
  return {
    link,
    ticket,
    factory,
    sockets,
    logger,
    flush,
    socket: () => sockets.at(-1)!,
    /** Start the link and complete a fresh handshake with one full `hosts` snapshot. */
    async online(hosts: object[] = [HOST]) {
      link.start();
      await flush();
      const socket = sockets.at(-1)!;
      socket.open();
      socket.welcome();
      socket.hosts(1, hosts);
      return socket;
    },
  };
}

/**
 * A link to a center that answers each new connection with a welcome and a full `hosts` snapshot
 * of `hosts()`, like the current center. While `center.reachable` is false, connections fail;
 * while `center.auto` is false, tests drive `sockets` themselves.
 */
export function centerLink(
  ticket: () => Promise<HubTicket>,
  hosts: () => object[],
  options: Partial<HubLinkOptions> = {},
) {
  const sockets: FakeHubSocket[] = [];
  const center = { auto: true, reachable: true };
  let links = 0;
  let sessions = 0;
  const link = new HubLink({
    ticket,
    webSocketFactory: (url, socketOptions) => {
      if (!center.reachable) throw new Error("offline");
      const socket = new FakeHubSocket(url, socketOptions);
      sockets.push(socket);
      if (center.auto)
        queueMicrotask(() => {
          socket.open();
          socket.welcome({ link: `link-${++links}` });
          socket.hosts(1, hosts());
        });
      return socket;
    },
    randomUUID: () => `session-${++sessions}`,
    logger: { info: vi.fn(), warn: vi.fn() },
    ...options,
  });
  return { link, sockets, center, socket: () => sockets.at(-1)! };
}

/**
 * A global `WebSocket` stand-in for the production adapters (`new WebSocket(url, protocols,
 * { headers })`), answering each connection like the current center: a welcome enabling every
 * announced feature and a full `hosts` snapshot of `HubWebSocket.hosts`. Call `reset()` per test.
 */
export class HubWebSocket extends FakeHubSocket {
  static instances: HubWebSocket[] = [];
  static hosts: object[] = [];
  private static links = 0;

  static reset(hosts: object[] = []): void {
    HubWebSocket.instances = [];
    HubWebSocket.hosts = hosts;
    HubWebSocket.links = 0;
  }
  /** Answer pings, like a connection that is alive. */
  answerPings = true;

  constructor(url: string, protocols?: string[], options?: { headers?: Record<string, string> }) {
    super(url, { protocols, headers: options?.headers });
    HubWebSocket.instances.push(this);
    queueMicrotask(() => {
      this.open();
      const resume = this.frames("hello").at(-1)?.resume as { link: string } | null | undefined;
      if (resume) {
        this.welcome({ resumed: true, link: resume.link, resume_token: undefined });
        return;
      }
      this.welcome({ link: `link-${++HubWebSocket.links}` });
      this.hosts(1, HubWebSocket.hosts);
    });
  }

  override send(data: string | Uint8Array | ArrayBuffer): void {
    super.send(data);
    const ping = this.sent.at(-1)!.header;
    if (this.answerPings && ping.t === "ping") this.receive({ t: "pong", n: ping.n });
  }
}
