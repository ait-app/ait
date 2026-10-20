import { WebSocket, type RawData } from "ws";
import type { AccountSessionManager } from "./account-session.js";

const MAX_PENDING_MESSAGES = 16;
const MAX_BUFFERED_MESSAGES = 65_536;
const MAX_BUFFERED_BYTES = 4 * 1024 * 1024;

/** Renderer or main-process consumer of an authenticated relay connection. */
export interface AccountTransportOwner {
  isDestroyed(): boolean;
  send(channel: string, payload: Record<string, unknown>): void;
  once(event: "destroyed", listener: () => void): unknown;
  on(event: "did-start-navigation", listener: Session["navigated"]): unknown;
  removeListener(event: "destroyed", listener: () => void): unknown;
  removeListener(event: "did-start-navigation", listener: Session["navigated"]): unknown;
}

interface Session {
  owner: AccountTransportOwner;
  destroyed: () => void;
  navigated: (_event: unknown, _url: string, sameDocument: boolean, mainFrame: boolean) => void;
  socket: WebSocket | null;
  visit: string | null;
  ready: boolean;
  verified: boolean;
  closed: boolean;
  expected: { serverId: string; instanceId: string } | null;
  deadline: ReturnType<typeof setTimeout>;
  pending: Map<number, { bytes: number; timer: ReturnType<typeof setTimeout> }>;
  queued: Map<number, { bytes: Buffer; binary: boolean }>;
  bufferedBytes: number;
  flushing: boolean;
  sequence: number;
}

/** Owns renderer transport sessions without exposing account or relay credentials. */
export class AccountTransportManager {
  private sessions = new Map<string, Session>();
  constructor(private readonly account: AccountSessionManager) {}

  private emit(id: string, session: Session, payload: Record<string, unknown>): void {
    if (!session.owner.isDestroyed())
      session.owner.send("paseo:event:account-relay-transport", { sessionId: id, ...payload });
  }

  async open(
    owner: AccountTransportOwner,
    id: string,
    host: string,
    center: string,
  ): Promise<void> {
    if (!/^account-[a-zA-Z0-9-]{1,80}$/.test(id) || !/^[0-9a-f-]{36}$/i.test(host))
      throw new Error("Invalid relay transport identity");
    if (this.sessions.has(id) || this.sessions.size >= 16)
      throw new Error("Too many relay transports");
    const session: Session = {
      owner,
      destroyed: () => this.finish(id, session),
      navigated: (_event, _url, sameDocument, mainFrame) => {
        if (mainFrame && !sameDocument) this.finish(id, session);
      },
      socket: null,
      visit: null,
      ready: false,
      verified: false,
      closed: false,
      expected: null,
      deadline: setTimeout(() => this.finish(id, session, "Relay setup timed out"), 45_000),
      pending: new Map(),
      queued: new Map(),
      bufferedBytes: 0,
      flushing: false,
      sequence: 0,
    };
    this.sessions.set(id, session);
    const destroyed = session.destroyed;
    owner.once("destroyed", destroyed);
    owner.on("did-start-navigation", session.navigated);
    try {
      const grant = await this.account.openVisit(host, center);
      if (session.closed) {
        await this.account.closeVisit(grant.relay_session_id);
        return;
      }
      session.visit = grant.relay_session_id;
      session.expected = { serverId: grant.server_id, instanceId: grant.instance_id };
      const socket = new WebSocket(grant.url, {
        headers: { Authorization: `Bearer ${grant.client_ticket}` },
        maxPayload: 1024 * 1024,
        perMessageDeflate: false,
        handshakeTimeout: 5000,
      });
      session.socket = socket;
      socket.on("message", (data, binary) => this.message(id, session, data, binary));
      socket.on("close", () => {
        owner.removeListener("destroyed", destroyed);
        this.finish(id, session);
      });
      socket.on("error", () => this.finish(id, session, "Relay connection failed"));
    } catch (error) {
      owner.removeListener("destroyed", destroyed);
      this.finish(id, session, error instanceof Error ? error.message : "Relay setup failed");
    }
  }

  private message(id: string, session: Session, data: RawData, binary: boolean): void {
    if (session.closed) return;
    const bytes = Buffer.isBuffer(data)
      ? data
      : data instanceof ArrayBuffer
        ? Buffer.from(data)
        : Buffer.concat(data);
    if (!session.ready || !session.verified) {
      let message: {
        type?: string;
        relay_session_id?: string;
        info?: { server_id?: string; instance_id?: string; features?: string[] };
      };
      try {
        if (binary) throw new Error();
        message = JSON.parse(bytes.toString());
      } catch {
        this.finish(id, session, "Invalid relay handshake");
        return;
      }
      if (!session.ready) {
        if (message.type !== "relay.ready" || message.relay_session_id !== session.visit) {
          this.finish(id, session, "Relay pairing rejected");
          return;
        }
        session.ready = true;
        clearTimeout(session.deadline);
        session.deadline = setTimeout(
          () => this.finish(id, session, "AIT hello timed out"),
          10_000,
        );
        this.emit(id, session, { kind: "open" });
        return;
      }
      if (
        message.type !== "server_info" ||
        message.info?.server_id !== session.expected?.serverId ||
        message.info?.instance_id !== session.expected?.instanceId ||
        !message.info?.features?.includes("ait-rust-single-v1")
      ) {
        this.finish(id, session, "Target Host identity or protocol changed");
        return;
      }
      session.verified = true;
      clearTimeout(session.deadline);
    }
    // pause() stops further socket reads, but ws can still dispatch frames from
    // its current read. Queue those frames separately from the IPC delivery window.
    if (
      session.pending.size + session.queued.size >= MAX_BUFFERED_MESSAGES ||
      session.bufferedBytes + bytes.length > MAX_BUFFERED_BYTES
    ) {
      this.finish(id, session, "Relay renderer backlog exceeded");
      return;
    }
    const sequence = ++session.sequence;
    session.queued.set(sequence, { bytes, binary });
    session.bufferedBytes += bytes.length;
    session.socket?.pause();
    this.flush(id, session);
  }

  private flush(id: string, session: Session): void {
    if (session.flushing || session.closed) return;
    session.flushing = true;
    try {
      for (const [sequence, { bytes, binary }] of session.queued) {
        if (session.closed || session.pending.size >= MAX_PENDING_MESSAGES) break;
        session.queued.delete(sequence);
        const timer = setTimeout(() => this.finish(id, session, "Relay renderer stalled"), 5000);
        session.pending.set(sequence, { bytes: bytes.length, timer });
        this.emit(id, session, {
          kind: "message",
          sequence,
          ...(binary ? { binaryBase64: bytes.toString("base64") } : { text: bytes.toString() }),
        });
      }
    } finally {
      session.flushing = false;
    }
    if (!session.closed && !session.pending.size && !session.queued.size) session.socket?.resume();
  }

  acknowledge(owner: AccountTransportOwner, id: string, sequence: number): void {
    const session = this.owned(owner, id);
    const pending = session.pending.get(sequence);
    if (!pending) return;
    clearTimeout(pending.timer);
    session.pending.delete(sequence);
    session.bufferedBytes -= pending.bytes;
    this.flush(id, session);
  }

  async send(
    owner: AccountTransportOwner,
    id: string,
    input: { text?: string; binaryBase64?: string },
  ): Promise<void> {
    const session = this.owned(owner, id);
    const socket = session.socket;
    if (!session.ready || !socket || socket.readyState !== WebSocket.OPEN)
      throw new Error("Relay is not ready");
    if ((input.text === undefined) === (input.binaryBase64 === undefined))
      throw new Error("Invalid relay frame");
    if ((input.text?.length ?? input.binaryBase64?.length ?? 0) > 2 * 1024 * 1024)
      throw new Error("Relay frame too large");
    const data = input.text === undefined ? Buffer.from(input.binaryBase64!, "base64") : input.text;
    if (
      Buffer.byteLength(data) > 1024 * 1024 ||
      socket.bufferedAmount + Buffer.byteLength(data) > 4 * 1024 * 1024
    ) {
      this.finish(id, session, "Relay send budget exceeded");
      throw new Error("Relay send budget exceeded");
    }
    await new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.finish(id, session, "Relay send timed out");
        reject(new Error("Relay send timed out"));
      }, 5000);
      socket.send(data, { binary: input.text === undefined }, (error) => {
        clearTimeout(timer);
        if (error) reject(error);
        else resolve();
      });
    });
  }

  close(owner: AccountTransportOwner, id: string): void {
    const session = this.sessions.get(id);
    if (!session) return;
    if (session.owner !== owner) throw new Error("Relay transport belongs to another window");
    this.finish(id, session);
  }

  closeAll(): void {
    for (const [id, session] of this.sessions) this.finish(id, session);
  }

  private owned(owner: AccountTransportOwner, id: string): Session {
    const session = this.sessions.get(id);
    if (!session || session.owner !== owner || session.closed)
      throw new Error("Relay transport is closed");
    return session;
  }

  private finish(id: string, session: Session, error?: string): void {
    if (session.closed) return;
    session.closed = true;
    session.owner.removeListener("destroyed", session.destroyed);
    session.owner.removeListener("did-start-navigation", session.navigated);
    clearTimeout(session.deadline);
    for (const pending of session.pending.values()) clearTimeout(pending.timer);
    session.pending.clear();
    session.queued.clear();
    session.bufferedBytes = 0;
    session.socket?.terminate();
    this.sessions.delete(id);
    if (session.visit) void this.account.closeVisit(session.visit);
    this.emit(id, session, { kind: "close", ...(error ? { error } : {}) });
  }
}
