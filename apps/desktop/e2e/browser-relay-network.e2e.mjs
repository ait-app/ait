// Run with Electron after building desktop main and target/debug/daemon.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { once } from "node:events";
import { mkdtempSync, rmSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { stripVTControlCharacters } from "node:util";
import { app, session } from "electron";
import { WebSocket, WebSocketServer } from "ws";
import { AccountTransportManager } from "../dist/daemon/account-transport.js";
import { BrowserRelayNetwork, browserProxyConfig } from "../dist/features/browser-network.js";

const temporary = mkdtempSync(path.join(tmpdir(), "ait-browser-relay-"));
app.setPath("userData", path.join(temporary, "electron"));
const root = fileURLToPath(new URL("../../../", import.meta.url));
const token = randomBytes(32).toString("hex");
let daemon;
let network;
let relay;
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
    const visit = "fixture-visit";
    let visits = 0;
    relay = new WebSocketServer({ host: "127.0.0.1", port: 0 });
    await once(relay, "listening");
    relay.on("connection", (client) => {
      sockets.add(client);
      const upstream = new WebSocket(`ws://${address}/v1/ws`, {
        headers: { Authorization: `Bearer ${token}` },
      });
      sockets.add(upstream);
      upstream.on("open", () =>
        client.send(JSON.stringify({ type: "relay.ready", relay_session_id: visit })),
      );
      upstream.on("message", (data, binary) => client.send(data, { binary }));
      client.on("message", (data, binary) => upstream.send(data, { binary }));
      client.on("close", () => upstream.terminate());
      upstream.on("close", () => client.terminate());
      upstream.on("error", () => client.terminate());
      client.on("error", () => upstream.terminate());
    });
    const account = {
      async openVisit() {
        visits++;
        return {
          relay_session_id: visit,
          url: `ws://127.0.0.1:${relay.address().port}`,
          client_ticket: "fixture",
          server_id: info.server_id,
          instance_id: info.instance_id,
        };
      },
      async closeVisit() {},
    };
    const payload = Buffer.alloc(200_000, "r");
    webpage = createServer((_request, response) => {
      response.setHeader("Content-Length", payload.length);
      response.end(payload);
    });
    webpage.listen(0, "127.0.0.1");
    await once(webpage, "listening");
    network = new BrowserRelayNetwork(new AccountTransportManager(account));
    const port = await network.listen(
      "00000000-0000-4000-8000-000000000001",
      "https://fixture.test",
      info.server_id,
    );
    const profile = session.fromPartition("browser-relay-test");
    await profile.setProxy(browserProxyConfig(port));
    const url = `http://127.0.0.1:${webpage.address().port}/`;
    for (const scheme of ["http", "https", "ws", "wss"]) {
      assert.equal(
        await profile.resolveProxy(url.replace("http", scheme)),
        `SOCKS5 127.0.0.1:${port}`,
      );
    }
    for (const host of ["localhost", "app.localhost", "127.2.3.4", "[::1]"]) {
      assert.equal(await profile.resolveProxy(`http://${host}:4322/`), `SOCKS5 127.0.0.1:${port}`);
    }
    for (const host of [
      "example.com",
      "localhost.example.com",
      "192.168.1.1",
      "169.254.1.1",
      "[fe80::1]",
    ]) {
      assert.equal(await profile.resolveProxy(`http://${host}/`), "DIRECT", host);
    }
    const response = await profile.fetch(url);
    assert.equal(response.status, 200);
    assert.deepEqual(Buffer.from(await response.arrayBuffer()), payload);
    assert.equal(visits, 1, "Chromium must use the authenticated relay, not client loopback");
    // Losing the visit closes active streams; refreshing establishes another authenticated visit.
    for (const socket of sockets) socket.terminate();
    await new Promise((resolve) => setTimeout(resolve, 50));
    await profile.closeAllConnections();
    assert.equal((await profile.fetch(url)).status, 200);
    assert.equal(visits, 2);
    console.log(
      "PASS: Electron loopback HTTP and 200 KB body through authenticated Rust relay; reconnect; HTTP(S)/WebSocket proxy selection",
    );
  } catch (error) {
    console.error(error);
    process.exitCode = 1;
  } finally {
    clearTimeout(deadline);
    network?.close();
    for (const socket of sockets) socket.terminate();
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
