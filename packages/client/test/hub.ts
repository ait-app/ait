import { vi } from "vitest";
import type { WebSocketLike } from "../src/daemon-client-transport-types.js";
import { HubLink, type HubLinkOptions } from "../src/hub/link.js";

export const CENTER = "https://center.test/api";
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
      ...overrides,
    });
  }

  presence(seq: number, nodes: object[]): void {
    this.receive({ t: "presence", seq }, JSON.stringify({ nodes }));
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
    /** Start the link and complete a fresh handshake with one presence snapshot. */
    async online(nodes: object[] = [HOST]) {
      link.start();
      await flush();
      const socket = sockets.at(-1)!;
      socket.open();
      socket.welcome();
      socket.presence(1, nodes);
      return socket;
    },
  };
}
