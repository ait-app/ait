#!/usr/bin/env node
const { appendFileSync, readFileSync, writeFileSync } = require("node:fs");
const path = require("node:path");
const { WebSocketServer } = require("ws");

const args = process.argv.slice(2);
const home = args[args.indexOf("--data-dir") + 1];
const listen = args[args.indexOf("--listen") + 1];
const config = () => JSON.parse(readFileSync(path.join(home, "readiness.json"), "utf8"));
const token = process.env.AIT_SERVER_TOKEN ?? "t".repeat(64);
writeFileSync(path.join(home, "local-api.token"), token, { mode: 0o600 });
appendFileSync(path.join(home, "pids"), `${process.pid}\n`);

if (config().exitBeforeReady) process.exit(1);
const server = new WebSocketServer({
  host: "127.0.0.1",
  port: Number(listen.split(":").at(-1)),
  path: "/v1/ws",
});
server.on("listening", () => {
  const report = () => {
    if (config().waitForReady) return;
    clearInterval(timer);
    process.stderr.write(`daemon ready listen=${listen}\n`);
  };
  const timer = setInterval(report, 10);
  report();
});
server.on("connection", (ws, request) => {
  if (request.headers.authorization !== `Bearer ${token}`) {
    ws.close();
    return;
  }
  ws.on("message", (data) => {
    const message = JSON.parse(data.toString());
    const settings = config();
    if (message.type === "hello") {
      if (settings.silent) return;
      if (settings.closeHello) return ws.close();
      if (settings.rejectHello) {
        ws.send(JSON.stringify({ type: "error", code: "forbidden", request_id: null }));
        return;
      }
      ws.send(
        JSON.stringify({
          type: "server_info",
          info: { server_id: "readiness-fixture", version: "fixture" },
        }),
      );
      return;
    }
    appendFileSync(path.join(home, "requests"), `${message.method}\n`);
    if (message.method === "server.shutdown.request") {
      ws.send(JSON.stringify({ type: "response", request_id: message.request_id, result: {} }));
      setImmediate(() => process.exit(0));
      return;
    }
    if (settings.busy) {
      ws.send(
        JSON.stringify({
          type: "error",
          request_id: message.request_id,
          code: "resource_exhausted",
          retryable: true,
        }),
      );
    } else {
      ws.send(
        JSON.stringify({
          type: "response",
          request_id: message.request_id,
          result: { pid: settings.wrongPid ? process.pid + 1 : process.pid },
        }),
      );
    }
  });
});
process.on("SIGTERM", () => {
  appendFileSync(path.join(home, "terminations"), `${process.pid}\n`);
  process.exit(0);
});
