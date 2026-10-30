import type { DaemonTransport } from "../daemon-client-transport-types.js";
import { HUB_MAX_PAYLOAD, type HubInboundHeader } from "./frame.js";

export type HubSessionHeader =
  | { t: "open"; s: string; to: { node: string }; share?: string }
  | { t: "msg"; s: string; pt: "text" | "binary" }
  | { t: "close"; s: string; code: string };

/** Link operations a session needs; implemented by `HubLink`. */
export interface HubSessionLink {
  recoverLiveness(): Promise<boolean>;
  send(header: HubSessionHeader, payload?: Uint8Array): void;
  release(session: string): void;
}

/** The center or peer ended the session; `code` is the hub close code, e.g. `share_revoked`. */
export class HubSessionClosedError extends Error {
  constructor(readonly code: string) {
    super(`Hub session closed (${code}).`);
    this.name = "HubSessionClosedError";
  }
}

/** Close event a session reports; `hubCode` is set when the center sent the close. */
export interface HubSessionCloseEvent {
  code: number;
  reason?: string;
  hubCode?: string;
}

const ACCEPT_TIMEOUT_MS = 30_000;
const encoder = new TextEncoder();
const decoder = new TextDecoder();

function bytes(data: string | Uint8Array | ArrayBuffer): Uint8Array {
  if (typeof data === "string") return encoder.encode(data);
  return data instanceof ArrayBuffer ? new Uint8Array(data) : data;
}

/**
 * One logical hub session carrying a daemon WebSocket connection. The first send is the business
 * hello and opens the session; the serve endpoint's accept payload is the business server_info.
 * Link reconnects within the grace period are invisible here: the session stays open and the
 * link replays frames in order.
 */
export class HubSession implements DaemonTransport {
  private readonly opens = new Set<() => void>();
  private readonly closes = new Set<(event?: unknown) => void>();
  private readonly errors = new Set<(event?: unknown) => void>();
  private readonly messages = new Set<(data: unknown, isBinary: boolean) => void>();
  private stage: "hello" | "accept" | "ready" | "closed" = "hello";
  private timer: ReturnType<typeof setTimeout> | null = null;
  private code: string | null = null;

  constructor(
    private readonly link: HubSessionLink,
    readonly id: string,
    readonly node: string,
    /** Share ID for a guest session to a resource another account shares. */
    readonly share?: string,
  ) {
    // Handlers are attached synchronously after construction; announce readiness afterwards.
    queueMicrotask(() => {
      if (this.stage === "hello") for (const handler of this.opens) handler();
    });
  }

  get closed(): boolean {
    return this.stage === "closed";
  }

  /**
   * Hub close code when the center ended the session (`peer_gone`, `unknown_share`,
   * `share_revoked`, ...), otherwise null. Guest sessions closed with `share_revoked` or
   * `unknown_share` should not be reopened automatically.
   */
  get closeCode(): string | null {
    return this.code;
  }

  /** Keep this session while the link resumes, then retry the daemon probe. */
  async recoverLiveness(): Promise<boolean> {
    if (this.closed) return false;
    const recovered = await this.link.recoverLiveness();
    return !this.closed && recovered;
  }

  /** The open frame was sent, so the peer may hold resources for this session. */
  get opened(): boolean {
    return this.stage === "accept" || this.stage === "ready";
  }

  send(data: string | Uint8Array | ArrayBuffer): void {
    if (this.stage === "closed") throw new Error("Hub session is closed.");
    const payload = bytes(data);
    if (payload.byteLength > HUB_MAX_PAYLOAD) {
      this.abort(new Error("Hub frame exceeds the size limit."));
      throw new Error("Hub frame too large.");
    }
    if (this.stage === "hello") {
      if (typeof data !== "string") throw new Error("Expected the AIT hello before sending data.");
      this.link.send(
        this.share
          ? { t: "open", s: this.id, to: { node: this.node }, share: this.share }
          : { t: "open", s: this.id, to: { node: this.node } },
        payload,
      );
      this.stage = "accept";
      this.timer = setTimeout(
        () => this.abort(new Error("Hub session was not accepted in time.")),
        ACCEPT_TIMEOUT_MS,
      );
      return;
    }
    this.link.send(
      { t: "msg", s: this.id, pt: typeof data === "string" ? "text" : "binary" },
      payload,
    );
  }

  /** Business close: tell the peer and release the session without emitting events. */
  close(): void {
    if (this.stage === "closed") return;
    const opened = this.stage !== "hello";
    this.dispose();
    if (opened) {
      try {
        this.link.send({ t: "close", s: this.id, code: "closed" });
      } catch {
        /* A stopped link already ended every session. */
      }
    }
  }

  /** End the session from the link side: link reset, sign-out or a closed peer. */
  fail(error?: Error): void {
    if (this.stage === "closed") return;
    this.dispose();
    if (error) for (const handler of this.errors) handler(error);
    const event: HubSessionCloseEvent = { code: error ? 1006 : 1000, reason: error?.message };
    if (error instanceof HubSessionClosedError) event.hubCode = error.code;
    for (const handler of this.closes) handler(event);
  }

  /** End the session locally (sign-out, suspension or a local failure) and tell the peer. */
  abort(error?: Error): void {
    if (this.stage === "closed") return;
    if (this.stage !== "hello") {
      try {
        this.link.send({ t: "close", s: this.id, code: error ? "failed" : "closed" });
      } catch {
        /* A stopped link already ended every session. */
      }
    }
    this.fail(error);
  }

  /** Called by the link for sequenced frames addressed to this session. */
  receive(header: HubInboundHeader, payload: Uint8Array): void {
    if (this.stage === "closed") return;
    if (header.t === "close") {
      // The center already ended the session; never echo a close back.
      this.code = header.code;
      this.fail(new HubSessionClosedError(header.code));
      return;
    }
    if (header.t === "accept") {
      if (this.stage !== "accept") {
        this.abort(new Error("Unexpected hub session accept."));
        return;
      }
      this.stage = "ready";
      if (this.timer) clearTimeout(this.timer);
      this.timer = null;
      if (payload.byteLength) this.deliver(decoder.decode(payload), false);
      return;
    }
    if (header.t === "msg") {
      if (header.pt === "text") this.deliver(decoder.decode(payload), false);
      else this.deliver(payload.slice(), true);
    }
  }

  private deliver(data: string | Uint8Array, binary: boolean): void {
    for (const handler of this.messages) {
      if (this.stage === "closed") return;
      handler(data, binary);
    }
  }

  private dispose(): void {
    this.stage = "closed";
    this.release();
  }

  private release(): void {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    this.link.release(this.id);
  }

  onOpen(handler: () => void): () => void {
    this.opens.add(handler);
    return () => {
      this.opens.delete(handler);
    };
  }

  onClose(handler: (event?: unknown) => void): () => void {
    this.closes.add(handler);
    return () => {
      this.closes.delete(handler);
    };
  }

  onError(handler: (event?: unknown) => void): () => void {
    this.errors.add(handler);
    return () => {
      this.errors.delete(handler);
    };
  }

  onMessage(handler: (data: unknown, isBinary: boolean) => void): () => void {
    this.messages.add(handler);
    return () => {
      this.messages.delete(handler);
    };
  }
}
