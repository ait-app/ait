import { EventEmitter, once } from "node:events";
import { randomUUID } from "node:crypto";
import { createServer, type Server, type Socket } from "node:net";
import type {
  AccountTransportManager,
  AccountTransportOwner,
} from "../daemon/account-transport.js";

const METHODS = [
  "browser.tunnel.open.request",
  "browser.tunnel.read.request",
  "browser.tunnel.write.request",
  "subscription.release.request",
];
const MAX_CHUNK = 64 * 1024;
const MAX_CONNECTIONS = 16;

/** Every destination uses the Workspace daemon, including DNS resolution. */
export function browserProxyConfig(port: number) {
  return {
    mode: "fixed_servers" as const,
    proxyRules: `socks5://127.0.0.1:${port}`,
    // Remove Chromium's implicit bypass for loopback and link-local addresses.
    proxyBypassRules: "<-loopback>",
  };
}

interface Pending {
  resolve(value: Record<string, unknown>): void;
  reject(error: Error): void;
  timer: ReturnType<typeof setTimeout>;
}

/** Multiplexes bounded TCP streams through an existing authenticated account transport. */
export class BrowserRelayNetwork extends EventEmitter implements AccountTransportOwner {
  private readonly id = `account-browser-${randomUUID()}`;
  private pending = new Map<string, Pending>();
  private sockets = new Set<Socket>();
  private server: Server | null = null;
  private closed = false;
  private ready: Promise<void> | null = null;
  private readyResolve: (() => void) | null = null;
  private readyReject: ((error: Error) => void) | null = null;
  private target: { hostId: string; center: string; serverId?: string } | null = null;

  constructor(
    private readonly transports: Pick<
      AccountTransportManager,
      "open" | "send" | "acknowledge" | "close"
    >,
  ) {
    super();
  }

  isDestroyed(): boolean {
    return this.closed;
  }

  send(_channel: string, payload: Record<string, unknown>): void {
    if (this.closed) return;
    if (payload.kind === "close" || payload.kind === "error") {
      this.disconnect(
        new Error(typeof payload.error === "string" ? payload.error : "Browser relay closed"),
      );
      return;
    }
    if (payload.kind === "open") {
      void this.transports
        .send(this, this.id, {
          text: JSON.stringify({
            type: "hello",
            client_id: this.id,
            protocol: { major: 1, min_minor: 0, max_minor: 0 },
            capabilities: METHODS,
            required_capabilities: ["connection.single.v1", ...METHODS],
          }),
        })
        .catch((error: Error) => this.disconnect(error));
      return;
    }
    if (payload.kind !== "message" || typeof payload.text !== "string") return;
    try {
      const message = JSON.parse(payload.text) as Record<string, unknown>;
      if (message.type === "server_info") {
        const info = message.info as { server_id?: string } | undefined;
        if (this.target?.serverId && info?.server_id !== this.target.serverId)
          throw new Error("Browser relay reached a different Workspace Host");
        this.readyResolve?.();
      } else {
        const request = this.pending.get(String(message.request_id));
        if (request) {
          this.pending.delete(String(message.request_id));
          clearTimeout(request.timer);
          if (message.type === "response")
            request.resolve(message.result as Record<string, unknown>);
          else request.reject(new Error(String(message.message ?? message.code)));
        } else if (message.type === "error") {
          this.disconnect(new Error("Remote Host must be updated to support browser forwarding"));
        }
      }
      this.transports.acknowledge(this, this.id, Number(payload.sequence));
    } catch (error) {
      this.disconnect(error instanceof Error ? error : new Error("Invalid browser relay response"));
    }
  }

  async listen(hostId: string, center: string, serverId?: string): Promise<number> {
    this.target = { hostId, center, serverId };
    this.server = createServer({ allowHalfOpen: true }, (socket) => {
      if (this.sockets.size >= MAX_CONNECTIONS) {
        socket.destroy();
        return;
      }
      this.sockets.add(socket);
      socket.on("error", () => undefined);
      socket.once("close", () => this.sockets.delete(socket));
      void this.accept(socket).catch(() => socket.destroy());
    });
    this.server.listen(0, "127.0.0.1");
    await once(this.server, "listening");
    const address = this.server.address();
    if (!address || typeof address === "string") throw new Error("Browser proxy did not listen");
    return address.port;
  }

  close(error = new Error("Browser network closed")): void {
    if (this.closed) return;
    this.closed = true;
    this.server?.close();
    this.disconnect(error);
    this.emit("closed");
  }

  private disconnect(error: Error): void {
    const active = this.ready !== null;
    this.readyReject?.(error);
    this.ready = null;
    this.readyResolve = null;
    this.readyReject = null;
    for (const socket of this.sockets) socket.destroy();
    for (const request of this.pending.values()) {
      clearTimeout(request.timer);
      request.reject(error);
    }
    this.pending.clear();
    if (active) this.transports.close(this, this.id);
  }

  private connect(): Promise<void> {
    if (!this.ready) {
      if (this.closed || !this.target) return Promise.reject(new Error("Browser network closed"));
      const ready = new Promise<void>((resolve, reject) => {
        this.readyResolve = resolve;
        this.readyReject = reject;
      });
      this.ready = ready;
      void this.transports
        .open(this, this.id, this.target.hostId, this.target.center)
        .catch((error: Error) => this.disconnect(error));
      return ready;
    }
    return this.ready;
  }

  private async rpc(
    method: string,
    params: Record<string, unknown>,
  ): Promise<Record<string, unknown>> {
    if (this.closed) throw new Error("Browser relay is closed");
    if (this.pending.size >= 64) throw new Error("Browser relay request budget exceeded");
    const requestId = randomUUID();
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.disconnect(new Error("Browser relay request timed out"));
      }, 40_000);
      this.pending.set(requestId, { resolve, reject, timer });
      void this.transports
        .send(this, this.id, {
          text: JSON.stringify({
            type: "request",
            request_id: requestId,
            method,
            params,
          }),
        })
        .catch((error: Error) => {
          clearTimeout(timer);
          this.pending.delete(requestId);
          reject(error);
          this.disconnect(error);
        });
    });
  }

  private async accept(socket: Socket): Promise<void> {
    // One iterator owns the socket, including bytes pipelined after the SOCKS handshake.
    const iterator = socket.iterator({ destroyOnReturn: false });
    let buffered = Buffer.alloc(0);
    const take = async (count: number): Promise<Buffer> => {
      while (buffered.length < count) {
        const next = await iterator.next();
        if (next.done) throw new Error("Incomplete browser proxy handshake");
        buffered = Buffer.concat([buffered, next.value as Buffer]);
        if (buffered.length > MAX_CHUNK) throw new Error("Browser proxy handshake too large");
      }
      const result = buffered.subarray(0, count);
      buffered = buffered.subarray(count);
      return result;
    };
    socket.setTimeout(70_000, () => socket.destroy());
    const greeting = await take(2);
    if (greeting[0] !== 5 || !(await take(greeting[1]!)).includes(0))
      throw new Error("Unsupported SOCKS authentication");
    socket.write(Buffer.from([5, 0]));
    const request = await take(4);
    if (request[0] !== 5 || request[1] !== 1 || request[2] !== 0)
      throw new Error("Only SOCKS CONNECT is supported");
    let host: string;
    if (request[3] === 1) host = [...(await take(4))].join(".");
    else if (request[3] === 3) host = (await take((await take(1))[0]!)).toString();
    else if (request[3] === 4) {
      const address = await take(16);
      host = Array.from({ length: 8 }, (_, index) =>
        address.readUInt16BE(index * 2).toString(16),
      ).join(":");
    } else throw new Error("Unsupported SOCKS address");
    const port = (await take(2)).readUInt16BE();
    if (!host || port === 0) throw new Error("Invalid browser destination");
    await this.connect();
    if (socket.destroyed) return;
    const opened = await this.rpc("browser.tunnel.open.request", { host, port });
    const subscriptionId = String(opened.subscriptionId);
    const release = () => {
      void this.rpc("subscription.release.request", { subscriptionId }).catch(() => undefined);
    };
    socket.once("close", release);
    try {
      if (socket.destroyed) return;
      socket.setTimeout(0);
      socket.write(Buffer.from([5, 0, 0, 1, 0, 0, 0, 0, 0, 0]));
      const writeRemote = async (data: Buffer, eof = false) => {
        for (let start = 0; start < data.length; start += MAX_CHUNK) {
          await this.rpc("browser.tunnel.write.request", {
            subscriptionId,
            dataBase64: data.subarray(start, start + MAX_CHUNK).toString("base64"),
          });
        }
        if (eof)
          await this.rpc("browser.tunnel.write.request", {
            subscriptionId,
            dataBase64: "",
            eof: true,
          });
      };
      const upload = async () => {
        await writeRemote(buffered);
        for await (const data of iterator) await writeRemote(data as Buffer);
        await writeRemote(Buffer.alloc(0), true);
      };
      const download = async () => {
        while (!socket.destroyed) {
          const data = await this.rpc("browser.tunnel.read.request", { subscriptionId });
          if (socket.destroyed) return;
          if (typeof data.dataBase64 !== "string" || typeof data.eof !== "boolean")
            throw new Error("Invalid browser stream");
          const bytes = Buffer.from(data.dataBase64, "base64");
          if (bytes.length > MAX_CHUNK) throw new Error("Browser stream chunk too large");
          if (bytes.length && !socket.write(bytes)) {
            await new Promise<void>((resolve) => {
              const done = () => {
                socket.removeListener("drain", done);
                socket.removeListener("close", done);
                resolve();
              };
              socket.once("drain", done);
              socket.once("close", done);
            });
          }
          if (data.eof) {
            socket.end();
            return;
          }
        }
      };
      await Promise.all([upload(), download()]);
    } finally {
      socket.removeListener("close", release);
      release();
      socket.destroy();
    }
  }
}
