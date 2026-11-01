#!/usr/bin/env node
const { readFileSync, writeFileSync } = require("node:fs");
const path = require("node:path");
const { WebSocketServer } = require("ws");

const args = process.argv.slice(2);
const home = args[args.indexOf("--data-dir") + 1];
const listen = args[args.indexOf("--listen") + 1];
const config = JSON.parse(readFileSync(path.join(home, "readiness.json"), "utf8"));
const requests = [];
writeFileSync(path.join(home, "daemon.pid"), String(process.pid));
writeFileSync(path.join(home, "local-api.token"), "t".repeat(64), { mode: 0o600 });

const server = new WebSocketServer({
  host: "127.0.0.1",
  port: Number(listen.split(":").at(-1)),
  path: "/v1/ws",
});
server.on("listening", () => process.stderr.write(`daemon ready listen=${listen}\n`));
server.on("connection", (ws, request) => {
  if (
    request.headers.authorization !== `Bearer ${process.env.AIT_SERVER_TOKEN ?? "t".repeat(64)}`
  ) {
    ws.close();
    return;
  }
  ws.on("message", (data) => {
    const message = JSON.parse(data.toString());
    if (message.type === "hello") {
      if (config.rejectHello) {
        ws.send(JSON.stringify({ type: "error", request_id: null, ...config.errors[0] }));
      } else {
        ws.send(
          JSON.stringify({
            type: "server_info",
            info: { server_id: "readiness-fixture", version: "fixture" },
          }),
        );
      }
      return;
    }
    if (message.method === "server.shutdown.request") {
      ws.send(JSON.stringify({ type: "response", request_id: message.request_id, result: {} }));
      setImmediate(() => process.exit(0));
      return;
    }
    requests.push(message);
    writeFileSync(path.join(home, "requests.json"), JSON.stringify(requests));
    const error =
      config.errors[requests.length - 1] ?? (config.repeatLastError ? config.errors.at(-1) : null);
    if (error) {
      ws.send(JSON.stringify({ type: "error", request_id: message.request_id, ...error }));
      if (config.closeAfterError) ws.close();
    } else {
      ws.send(
        JSON.stringify({
          type: "response",
          request_id: message.request_id,
          result: { pid: process.pid },
        }),
      );
    }
  });
});
process.on("SIGTERM", () => {
  writeFileSync(path.join(home, "termination"), "SIGTERM");
  process.exit(0);
});
