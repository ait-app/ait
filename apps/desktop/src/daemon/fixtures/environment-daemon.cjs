#!/usr/bin/env node
// Records the exact environment the Desktop launcher passed, then completes the same
// readiness handshake as a real daemon so RustDaemonManager.start() resolves.
const { appendFileSync, writeFileSync } = require("node:fs");
const path = require("node:path");
const { WebSocketServer } = require("ws");

const args = process.argv.slice(2);
const home = args[args.indexOf("--data-dir") + 1];
const listen = args[args.indexOf("--listen") + 1];
const token = process.env.AIT_SERVER_TOKEN ?? "t".repeat(64);
writeFileSync(path.join(home, "local-api.token"), token, { mode: 0o600 });
writeFileSync(path.join(home, "received-env.json"), JSON.stringify(process.env), { mode: 0o600 });
appendFileSync(path.join(home, "pids"), `${process.pid}\n`);

const server = new WebSocketServer({
  host: "127.0.0.1",
  port: Number(listen.split(":").at(-1)),
  path: "/v1/ws",
});
server.on("listening", () => {
  process.stderr.write(`daemon ready listen=${listen}\n`);
});
server.on("connection", (ws, request) => {
  if (request.headers.authorization !== `Bearer ${token}`) {
    ws.close();
    return;
  }
  ws.on("message", (data) => {
    const message = JSON.parse(data.toString());
    if (message.type === "hello") {
      ws.send(
        JSON.stringify({
          type: "server_info",
          info: { server_id: "environment-fixture", version: "fixture" },
        }),
      );
      return;
    }
    if (message.method === "server.shutdown.request") {
      ws.send(JSON.stringify({ type: "response", request_id: message.request_id, result: {} }));
      setImmediate(() => process.exit(0));
    }
  });
});
process.on("SIGTERM", () => process.exit(0));
