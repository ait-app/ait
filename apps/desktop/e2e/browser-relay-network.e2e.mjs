// Run with Electron after building desktop main and target/debug/daemon.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { randomBytes, randomUUID } from "node:crypto";
import { once } from "node:events";
import { mkdtempSync, rmSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { stripVTControlCharacters } from "node:util";
import { app, session } from "electron";
import { WebSocket, WebSocketServer } from "ws";
import { HubLink, encodeHubFrame } from "@ait/client/internal/hub";
import { AccountTransportManager } from "../dist/daemon/account-transport.js";
import { BrowserRelayNetwork, browserProxyConfig } from "../dist/features/browser-network.js";
import { BrowserDaemonTransport } from "../dist/features/browser-daemon-transport.js";

const temporary = mkdtempSync(path.join(tmpdir(), "ait-browser-relay-"));
app.setPath("userData", path.join(temporary, "electron"));
const root = fileURLToPath(new URL("../../../", import.meta.url));
const token = randomBytes(32).toString("hex");
let daemon;
let network;
let directNetwork;
let relay;
let hub;
let webpage;
const sockets = new Set();
let deadline;
app.whenReady().then(async () => {
  try {
    daemon = spawn(
      process.env.AIT_SERVER_BIN ?? path.join(root, "target/debug/daemon"),
      ["--data-dir", path.join(temporary, "daemon"), "--listen", "127.0.0.1:0"],
      { env: { ...process.env, AIT_SERVER_TOKEN: token }, stdio: ["ignore", "pipe", "pipe"] },
    );
    const address = await new Promise((resolve, reject) => {
      deadline = setTimeout(() => reject(new Error("Test daemon did not become ready")), 30_000);
      const receive = (data) => {
        const text = stripVTControlCharacters(data.toString());
        if (text.includes("daemon ready")) {
          const address = text.match(/127\.0\.0\.1:\d+/)?.[0];
          if (address) {
            clearTimeout(deadline);
            resolve(address);
          }
        }
      };
      daemon.stdout.on("data", receive);
      daemon.stderr.on("data", receive);
      daemon.once("error", reject);
      daemon.once("exit", (code) => reject(new Error(`Test daemon exited: ${code}`)));
    });
    const info = await (
      await fetch(`http://${address}/v1/server/info`, {
        headers: { Authorization: `Bearer ${token}` },
      })
    ).json();
    const hostId = "00000000-0000-4000-8000-000000000001";
    let visits = 0;
    relay = new WebSocketServer({ host: "127.0.0.1", port: 0 });
    await once(relay, "listening");
    const center = `http://127.0.0.1:${relay.address().port}`;
    // A minimal Hub center carrying real business frames to the Rust daemon.
    relay.on("connection", (client) => {
      sockets.add(client);
      let sequence = 0;
      const sessions = new Map();
      const send = (header, payload) => client.send(encodeHubFrame(header, payload));
      client.on("message", (raw) => {
        const length = raw.readUInt16BE();
        const header = JSON.parse(raw.subarray(2, 2 + length).toString());
        const payload = raw.subarray(2 + length);
        if (header.t === "hello") {
          send({
            t: "welcome",
            v: 1,
            link: randomUUID(),
            resumed: false,
            recv: 0,
            node_id: "access",
            grace_seconds: 1,
            resume_token: "r".repeat(64),
          });
          send(
            { t: "presence", seq: ++sequence },
            Buffer.from(
              JSON.stringify({
                nodes: [{ node_id: "serve", host_id: hostId, state: "online" }],
              }),
            ),
          );
        } else if (header.t === "ping") send({ t: "pong", n: header.n });
        else if (header.t === "open") {
          visits++;
          const upstream = new WebSocket(`ws://${address}/v1/ws`, {
            headers: { Authorization: `Bearer ${token}` },
          });
          sessions.set(header.s, upstream);
          sockets.add(upstream);
          let accepted = false;
          upstream.on("open", () => upstream.send(payload, { binary: false }));
          upstream.on("message", (data, binary) => {
            if (client.readyState !== WebSocket.OPEN) return;
            send(
              {
                t: accepted ? "msg" : "accept",
                seq: ++sequence,
                s: header.s,
                ...(accepted ? { pt: binary ? "binary" : "text" } : {}),
              },
              data,
            );
            accepted = true;
          });
          upstream.on("close", () => {
            if (client.readyState === WebSocket.OPEN)
              send({ t: "close", seq: ++sequence, s: header.s, code: "closed" });
          });
          upstream.on("error", () => client.terminate());
        } else if (header.t === "msg")
          sessions.get(header.s)?.send(payload, { binary: header.pt === "binary" });
        else if (header.t === "close") sessions.get(header.s)?.close();
      });
      client.on("close", () => {
        for (const upstream of sessions.values()) upstream.terminate();
      });
      client.on("error", () => client.terminate());
    });
    hub = new HubLink({
      ticket: async () => ({ centerUrl: center, ticket: "a".repeat(64) }),
      webSocketFactory: (url, options) =>
        new WebSocket(url, options?.protocols, { headers: options?.headers }),
    });
    hub.start();
    assert.equal(await hub.whenReady(5000), true);
    const account = {
      snapshot: () => ({ hosts: [{ host_id: hostId, server_id: info.server_id }] }),
    };
    const payload = Buffer.alloc(200_000, "r");
    webpage = createServer((_request, response) => {
      response.setHeader("Content-Length", payload.length);
      response.end(payload);
    });
    webpage.listen(0, "127.0.0.1");
    await once(webpage, "listening");
    network = new BrowserRelayNetwork(new AccountTransportManager(account, hub));
    const port = await network.listen(hostId, center, info.server_id);
    const profile = session.fromPartition("browser-relay-test");
    await profile.setProxy(browserProxyConfig(port));
    const url = `http://127.0.0.1:${webpage.address().port}/`;
    const localProfile = session.fromPartition("browser-local-test");
    await localProfile.setProxy({ mode: "direct" });
    for (const scheme of ["http", "https", "ws", "wss"]) {
      for (const host of [
        "localhost",
        "app.localhost",
        "127.2.3.4",
        "[::1]",
        "example.com",
        "localhost.example.com",
        "203.0.113.1",
        "192.168.1.1",
        "169.254.1.1",
        "[fe80::1]",
        "[2001:db8::1]",
      ]) {
        const destination = `${scheme}://${host}:4322/`;
        assert.equal(
          await profile.resolveProxy(destination),
          `SOCKS5 127.0.0.1:${port}`,
          destination,
        );
        assert.equal(await localProfile.resolveProxy(destination), "DIRECT", destination);
      }
    }
    const response = await profile.fetch(url);
    assert.equal(response.status, 200);
    assert.deepEqual(Buffer.from(await response.arrayBuffer()), payload);
    assert.equal((await profile.fetch(url.replace("127.0.0.1", "app.localhost"))).status, 200);
    assert.equal(visits, 1, "Chromium must use the authenticated Hub session");
    // Losing the logical Hub link closes streams; refreshing creates another session.
    for (const socket of sockets) socket.terminate();
    await new Promise((resolve) => setTimeout(resolve, 50));
    await profile.closeAllConnections();
    assert.equal((await profile.fetch(url)).status, 200);
    assert.equal(visits, 2);
    directNetwork = new BrowserRelayNetwork(
      new BrowserDaemonTransport({
        target: { transportType: "rustTcp", url: `ws://${address}/v1/ws` },
        bearerToken: token,
      }),
    );
    const directPort = await directNetwork.listen("", "", info.server_id);
    const directProfile = session.fromPartition("browser-direct-daemon-test");
    await directProfile.setProxy(browserProxyConfig(directPort));
    const directResponse = await directProfile.fetch(url.replace("127.0.0.1", "app.localhost"));
    assert.equal(directResponse.status, 200);
    assert.deepEqual(Buffer.from(await directResponse.arrayBuffer()), payload);
    await directProfile.closeAllConnections();
    await profile.closeAllConnections();
    console.log(
      "PASS: all-destination HTTP(S)/WebSocket proxy selection; local direct routing; authenticated Hub and TCP daemon HTTP with 200 KB bodies; reconnect",
    );
  } catch (error) {
    console.error(error);
    process.exitCode = 1;
  } finally {
    clearTimeout(deadline);
    network?.close();
    directNetwork?.close();
    for (const socket of sockets) socket.terminate();
    hub?.stop();
    relay?.close();
    webpage?.closeAllConnections();
    webpage?.close();
    if (daemon && daemon.exitCode === null) {
      daemon.kill("SIGTERM");
      await Promise.race([
        once(daemon, "exit"),
        new Promise((resolve) =>
          setTimeout(() => {
            daemon.kill("SIGKILL");
            resolve();
          }, 5_000),
        ),
      ]);
    }
    rmSync(temporary, { recursive: true, force: true });
    app.exit(process.exitCode ?? 0);
  }
});
