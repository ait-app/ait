import type { DaemonTransport } from "../daemon-client-transport-types.js";
import { HUB_MAX_PAYLOAD, type HubInboundHeader, type HubSessionKind } from "./frame.js";

export type HubSessionHeader =
  | {
      t: "open";
      s: string;
      to: { node: string };
      share?: string;
      k?: HubSessionKind;
      w?: number;
    }
  | { t: "msg"; s: string; pt: "text" | "binary" }
  | { t: "close"; s: string; code: string };

/** Link operations a session needs; implemented by `HubLink`. */
export interface HubSessionLink {
  recoverLiveness(): Promise<boolean>;
  send(header: HubSessionHeader, payload?: Uint8Array): void;
  /** Grant cumulative credit; unsequenced, so the session resends it after a resume. */
  credit(session: string, c: number): void;
  release(session: string): void;
}

/** The link's unacknowledged buffer cannot take this frame now; retry after acknowledgements. */
export class HubBufferFullError extends Error {
  constructor() {
    super("Hub send buffer is full.");
    this.name = "HubBufferFullError";
  }
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

export interface HubSessionOptions {
  /** Share ID for a guest session to a resource another account shares. */
  share?: string;
  kind?: HubSessionKind;
  /** This side's receive window, announced in `open.w` when the link enabled `credit`. */
  window?: number;
  /** Grant credit only for bytes reported through `consumed()`, e.g. after a disk write. */
  manualCredit?: boolean;
}

/** Messages queued while the peer window or link buffer is full; a stalled peer fails beyond it. */
const MAX_QUEUED_BYTES = 16 * 1024 * 1024;
const ACCEPT_TIMEOUT_MS = 30_000;
const encoder = new TextEncoder();
const decoder = new TextDecoder();

function bytes(data: string | Uint8Array | ArrayBuffer): Uint8Array {
  if (typeof data === "string") return encoder.encode(data);
  return data instanceof ArrayBuffer ? new Uint8Array(data) : data;
}

/** Per-session flow control state (`credit` feature). */
interface Credit {
  /** Our receive window, sent in `open.w`. */
  window: number;
  /** Peer window from `accept.w`; null until accepted. */
  peerWindow: number | null;
  /** Latest cumulative credit granted by the peer. */
  peer: number;
  /** Message payload bytes sent. */
  sent: number;
  /** Message payload bytes the consumer finished with. */
  delivered: number;
  /** Latest credit we granted. */
  granted: number;
}

/**
 * One logical hub session carrying a daemon WebSocket connection. The first send is the business
 * hello and opens the session; the serve endpoint's accept payload is the business server_info.
 * Link reconnects within the grace period are invisible here: the session stays open and the
 * link replays frames in order.
 *
 * With `credit`, sends wait for the peer's credit instead of overrunning its window, and the
 * session grants credit as the consumer receives messages. An accept without `w` (an older Host
 * or center) keeps the uncontrolled behavior.
 */
export class HubSession implements DaemonTransport {
  private readonly opens = new Set<() => void>();
  private readonly closes = new Set<(event?: unknown) => void>();
  private readonly errors = new Set<(event?: unknown) => void>();
  private readonly messages = new Set<(data: unknown, isBinary: boolean) => void>();
  private readonly drains = new Set<() => void>();
  private stage: "hello" | "accept" | "ready" | "closed" = "hello";
  private timer: ReturnType<typeof setTimeout> | null = null;
  private code: string | null = null;
  private credit: Credit | null;
  private queue: { pt: "text" | "binary"; payload: Uint8Array }[] = [];
  private queuedBytes = 0;
  /** Share ID for a guest session to a resource another account shares. */
  readonly share?: string;
  readonly kind: HubSessionKind;
  private readonly manualCredit: boolean;

  constructor(
    private readonly link: HubSessionLink,
    readonly id: string,
    readonly node: string,
    options: HubSessionOptions = {},
  ) {
    this.share = options.share;
    this.kind = options.kind ?? "rpc";
    this.manualCredit = options.manualCredit ?? false;
    this.credit = options.window
      ? { window: options.window, peerWindow: null, peer: 0, sent: 0, delivered: 0, granted: 0 }
      : null;
    // Handlers are attached synchronously after construction; announce readiness afterwards.
    queueMicrotask(() => {
      if (this.stage === "hello") for (const handler of this.opens) handler();
    });
  }

  get closed(): boolean {
    return this.stage === "closed";
  }

  /** Both sides announced a window, so this session is flow-controlled. */
  get flowControlled(): boolean {
    return this.credit?.peerWindow != null;
  }

  /** Message bytes waiting for peer credit or link buffer space. */
  get bufferedAmount(): number {
    return this.queuedBytes;
  }

  /**
   * Hub close code when the center ended the session (`peer_gone`, `unknown_share`,
   * `share_revoked`, `done`, `unsupported`, ...), otherwise null. Guest sessions closed with
   * `share_revoked` or `unknown_share` lost their access and should not be reopened
   * automatically; `peer_gone` means the sharing Host went away, so reopen once its share is
   * `online` again.
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
        {
          t: "open",
          s: this.id,
          to: { node: this.node },
          ...(this.share ? { share: this.share } : {}),
          ...(this.kind === "download" ? { k: "download" as const } : {}),
          ...(this.credit ? { w: this.credit.window } : {}),
        },
        payload,
      );
      this.stage = "accept";
      this.timer = setTimeout(
        () => this.abort(new Error("Hub session was not accepted in time.")),
        ACCEPT_TIMEOUT_MS,
      );
      return;
    }
    const pt = typeof data === "string" ? "text" : "binary";
    if (!this.credit) {
      this.link.send({ t: "msg", s: this.id, pt }, payload);
      return;
    }
    // Whether flow control applies is known only after the accept; queue until then.
    if (this.queuedBytes + payload.byteLength > MAX_QUEUED_BYTES) {
      this.abort(new Error("Hub session send buffer is full."));
      throw new Error("Hub session send buffer is full.");
    }
    this.queue.push({ pt, payload });
    this.queuedBytes += payload.byteLength;
    this.pump();
  }

  /** Resolve once queued messages were handed to the link, or the session ended. */
  drain(): Promise<void> {
    if (!this.queue.length || this.closed) return Promise.resolve();
    return new Promise((resolve) => {
      const done = () => {
        this.drains.delete(done);
        resolve();
      };
      this.drains.add(done);
    });
  }

  /** Send queued messages the peer window and link buffer admit; the link calls it after acks. */
  pump(): void {
    const credit = this.credit;
    if (this.stage !== "ready") return;
    while (this.queue.length) {
      const item = this.queue[0];
      const length = item.payload.byteLength;
      if (credit?.peerWindow != null) {
        if (length > credit.peerWindow) {
          this.abort(new Error("Hub message exceeds the peer window."));
          return;
        }
        if (credit.sent + length > credit.peer + credit.peerWindow) break;
      }
      try {
        this.link.send({ t: "msg", s: this.id, pt: item.pt }, item.payload);
      } catch (error) {
        if (error instanceof HubBufferFullError && credit) break;
        this.abort(error instanceof Error ? error : new Error("Hub session send failed."));
        return;
      }
      this.queue.shift();
      this.queuedBytes -= length;
      if (credit?.peerWindow != null) credit.sent += length;
    }
    if (!this.queue.length) for (const resolve of this.drains) resolve();
  }

  /**
   * Report message bytes the consumer finished with; grants credit every half window. Sessions
   * with `manualCredit` call it after processing, e.g. a download after writing to disk.
   */
  consumed(length: number): void {
    const credit = this.credit;
    if (credit?.peerWindow == null || this.closed) return;
    credit.delivered += length;
    if (credit.delivered - credit.granted < credit.window / 2) return;
    credit.granted = credit.delivered;
    this.link.credit(this.id, credit.granted);
  }

  /** Credit is unsequenced: after a link resume both sides resend their latest value. */
  resendCredit(): void {
    const credit = this.credit;
    if (credit?.peerWindow == null || this.closed || !credit.delivered) return;
    credit.granted = credit.delivered;
    this.link.credit(this.id, credit.granted);
  }

  /** Peer credit forwarded by the center. */
  receiveCredit(c: number): void {
    const credit = this.credit;
    if (credit?.peerWindow == null || this.closed || c <= credit.peer) return;
    credit.peer = c;
    this.pump();
  }

  /** Business close: tell the peer and release the session without emitting events. */
  close(): void {
    this.end("closed");
  }

  /** Close with a hub close code, e.g. `cancelled` for a download the user stopped. */
  end(code: string): void {
    if (this.stage === "closed") return;
    const opened = this.stage !== "hello";
    this.dispose();
    if (opened) {
      try {
        this.link.send({ t: "close", s: this.id, code });
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
  abort(error?: Error, code?: string): void {
    if (this.stage === "closed") return;
    if (this.stage !== "hello") {
      try {
        this.link.send({ t: "close", s: this.id, code: code ?? (error ? "failed" : "closed") });
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
      // An accept without a window means the Host or center has no `credit`.
      if (this.credit && header.w === undefined) this.credit = null;
      else if (this.credit) this.credit.peerWindow = header.w!;
      if (payload.byteLength) this.deliver(decoder.decode(payload), false);
      this.pump();
      return;
    }
    if (header.t === "msg") {
      if (header.pt === "text") this.deliver(decoder.decode(payload), false);
      else this.deliver(payload.slice(), true);
      if (!this.manualCredit) this.consumed(payload.byteLength);
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
    this.queue = [];
    this.queuedBytes = 0;
    for (const resolve of this.drains) resolve();
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
