import type { AccountSessionManager } from "./account-session.js";
import type { HubLink, HubSession } from "@ait/client/internal/hub";

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
  hub: HubSession | null;
  verified: boolean;
  closed: boolean;
  expectedServerId: string | null;
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
  constructor(
    private readonly account: AccountSessionManager,
    private readonly link?: HubLink,
  ) {}

  private emit(id: string, session: Session, payload: Record<string, unknown>): void {
    if (!session.owner.isDestroyed())
      session.owner.send("paseo:event:account-relay-transport", { sessionId: id, ...payload });
  }

  async open(
    owner: AccountTransportOwner,
    id: string,
    host: string,
    center: string,
    guest?: { shareId: string; nodeId: string },
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
      hub: null,
      verified: false,
      closed: false,
      expectedServerId: null,
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
      if (this.link?.pending) await this.link.whenReady(5000);
      const node =
        this.link?.ready && this.link.centerUrl === center
          ? guest
            ? { node_id: guest.nodeId }
            : this.link.serveFor(host)
          : undefined;
      if (session.closed) return;
      if (!this.link?.ready || this.link.centerUrl !== center)
        throw new Error("Online Hub is unavailable.");
      if (!node) throw new Error("Target Host is unavailable on Hub.");
      const target = guest
        ? null
        : this.account.snapshot().hosts.find((value) => value.host_id === host);
      if (!guest && !target?.server_id) throw new Error("Host identity unavailable");
      // Hub authenticates the shared serve node; guests do not have its owner Host record.
      session.expectedServerId = target?.server_id ?? null;
      const transport = this.link.openSession(node.node_id, guest ? { share: guest.shareId } : {});
      session.hub = transport;
      transport.onOpen(() => {
        if (!session.closed) this.emit(id, session, { kind: "open" });
      });
      transport.onMessage((data, binary) =>
        this.message(
          id,
          session,
          typeof data === "string" ? Buffer.from(data) : Buffer.from(data as Uint8Array),
          binary,
        ),
      );
      transport.onError(() => this.finish(id, session, "Hub session failed"));
      transport.onClose(() => this.finish(id, session));
    } catch (error) {
      owner.removeListener("destroyed", destroyed);
      this.finish(id, session, error instanceof Error ? error.message : "Relay setup failed");
    }
  }

  private message(id: string, session: Session, bytes: Buffer, binary: boolean): void {
    if (session.closed) return;
    if (!session.verified) {
      let message: {
        type?: string;
        info?: { server_id?: string; instance_id?: string; features?: string[] };
      };
      try {
        if (binary) throw new Error();
        message = JSON.parse(bytes.toString());
      } catch {
        this.finish(id, session, "Invalid relay handshake");
        return;
      }
      if (
        message.type !== "server_info" ||
        (session.expectedServerId !== null &&
          message.info?.server_id !== session.expectedServerId) ||
        typeof message.info?.instance_id !== "string" ||
        !message.info.instance_id ||
        !Array.isArray(message.info?.features) ||
        !message.info.features.includes("ait-rust-single-v1")
      ) {
        this.finish(id, session, "Target Host identity or protocol changed");
        return;
      }
      session.verified = true;
      clearTimeout(session.deadline);
    }
    // Bound Hub output independently of the renderer IPC delivery window.
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
    if (!session.hub) throw new Error("Hub session is not ready");
    if ((input.text === undefined) === (input.binaryBase64 === undefined))
      throw new Error("Invalid Hub frame");
    if ((input.text?.length ?? input.binaryBase64?.length ?? 0) > 2 * 1024 * 1024)
      throw new Error("Hub frame too large");
    session.hub.send(input.text ?? Buffer.from(input.binaryBase64!, "base64"));
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

  recover(owner: AccountTransportOwner, id: string): Promise<boolean> {
    return this.owned(owner, id).hub?.recoverLiveness() ?? Promise.resolve(false);
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
    session.hub?.close();
    this.sessions.delete(id);
    this.emit(id, session, { kind: "close", ...(error ? { error } : {}) });
  }
}
