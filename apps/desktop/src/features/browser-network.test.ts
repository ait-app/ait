import { connect, createServer, type Socket } from "node:net";
import { once } from "node:events";
import { afterEach, describe, expect, test } from "vitest";
import type { AccountTransportOwner } from "../daemon/account-transport.js";
import {
  BrowserRelayNetwork,
  browserProxyConfig,
  isBrowserLoopbackHost,
} from "./browser-network.js";

/** Emulates the authenticated daemon seam, deliberately mapping the target to a different listener. */
class RemoteTransport {
  opens = 0;
  released = 0;
  sockets = new Map<string, Socket>();
  async open(owner: AccountTransportOwner) {
    this.opens++;
    owner.send("relay", { kind: "open" });
  }
  constructor(private readonly port: number) {}
  acknowledge() {}
  close() {
    for (const socket of this.sockets.values()) socket.destroy();
    this.sockets.clear();
  }
  async send(owner: AccountTransportOwner, _id: string, input: { text?: string }) {
    const message = JSON.parse(input.text!);
    if (message.type === "hello") {
      owner.send("relay", { kind: "message", text: JSON.stringify({ type: "server_info" }) });
      return;
    }
    let result: Record<string, unknown> = {};
    const id = message.params.subscriptionId;
    if (message.method === "browser.tunnel.open.request") {
      expect(message.params).toEqual({ host: "127.0.0.1", port: 4322 });
      const socket = connect(this.port, "127.0.0.1");
      socket.on("error", () => undefined);
      await once(socket, "connect");
      const subscriptionId = `stream-${this.sockets.size}`;
      this.sockets.set(subscriptionId, socket);
      result = { subscriptionId };
    } else if (message.method === "browser.tunnel.write.request") {
      const socket = this.sockets.get(id)!;
      const data = Buffer.from(message.params.dataBase64, "base64");
      if (data.length)
        await new Promise<void>((resolve, reject) =>
          socket.write(data, (error) => (error ? reject(error) : resolve())),
        );
      if (message.params.eof) socket.end();
    } else if (message.method === "browser.tunnel.read.request") {
      const socket = this.sockets.get(id)!;
      let data = socket.read() as Buffer | null;
      while (data === null && !socket.readableEnded && !socket.destroyed) {
        await Promise.race([once(socket, "readable"), once(socket, "end"), once(socket, "close")]);
        data = socket.read() as Buffer | null;
      }
      result = { dataBase64: data?.toString("base64") ?? "", eof: data === null };
    } else if (message.method === "subscription.release.request") {
      this.released++;
      this.sockets.get(id)?.destroy();
      this.sockets.delete(id);
    } else throw new Error(`Unexpected method ${message.method}`);
    owner.send("relay", {
      kind: "message",
      text: JSON.stringify({ type: "response", request_id: message.request_id, result }),
    });
  }
}

const cleanup: (() => void)[] = [];
afterEach(() => {
  for (const close of cleanup.splice(0)) close();
});

async function proxiedSocket(proxyPort: number) {
  const socket = connect(proxyPort, "127.0.0.1");
  socket.on("error", () => undefined);
  cleanup.push(() => socket.destroy());
  await once(socket, "connect");
  socket.write(Buffer.from([5, 1, 0]));
  const [greeting] = await once(socket, "data");
  expect(greeting).toEqual(Buffer.from([5, 0]));
  // SOCKS destination is the remote Workspace's port, not the local fixture's port.
  socket.write(Buffer.from([5, 1, 0, 1, 127, 0, 0, 1, 0x10, 0xe2]));
  const [accepted] = await once(socket, "data");
  expect(accepted[1]).toBe(0);
  return socket;
}

describe("remote Workspace browser network", () => {
  test("routes only loopback through SOCKS and disables Chromium's implicit loopback bypass", () => {
    const config = browserProxyConfig(12345);
    for (const host of ["localhost", "app.localhost", "127.0.0.1", "127.2.3.4", "::1", "[::1]"]) {
      expect(isBrowserLoopbackHost(host)).toBe(true);
    }
    for (const host of ["example.com", "localhost.example.com", "192.168.1.1"]) {
      expect(isBrowserLoopbackHost(host)).toBe(false);
    }
    expect(config.proxyRules).toBe("socks5://127.0.0.1:12345");
    expect(config.proxyBypassRules).toBe("*;<-loopback>;169.254.0.0/16;fe80::/10");
  });

  test("loads the remote HTTP response and keeps WebSocket upgrade bytes intact", async () => {
    const requests: string[] = [];
    const remote = createServer((socket) =>
      socket.on("data", (data) => {
        requests.push(data.toString());
        socket.write(
          "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\nremote workspace",
        );
      }),
    );
    remote.listen(0, "127.0.0.1");
    await once(remote, "listening");
    cleanup.push(() => remote.close());
    const address = remote.address();
    if (!address || typeof address === "string") throw new Error("fixture not listening");
    const transport = new RemoteTransport(address.port);
    const network = new BrowserRelayNetwork(transport);
    cleanup.push(() => network.close());
    const proxy = await network.listen("host", "https://center.test");
    expect(transport.opens).toBe(0);
    const socket = await proxiedSocket(proxy);
    const response = once(socket, "data");
    socket.write("GET /live HTTP/1.1\r\nHost: 127.0.0.1:4322\r\nUpgrade: websocket\r\n\r\n");
    expect((await response)[0].toString()).toContain("101 Switching Protocols");
    expect(requests[0]).toContain("Host: 127.0.0.1:4322");
    socket.destroy();
    await new Promise<void>((resolve) => setImmediate(resolve));
    expect(transport.opens).toBe(1);
  });

  test("rejects arbitrary proxy destinations without creating a relay visit", async () => {
    const transport = new RemoteTransport(1);
    const network = new BrowserRelayNetwork(transport);
    cleanup.push(() => network.close());
    const proxy = await network.listen("host", "https://center.test");
    const socket = connect(proxy, "127.0.0.1");
    cleanup.push(() => socket.destroy());
    await once(socket, "connect");
    socket.write(Buffer.from([5, 1, 0, 5, 1, 0, 1, 10, 0, 0, 1, 0, 80]));
    socket.resume();
    await once(socket, "close");
    expect(transport.opens).toBe(0);
  });
});
