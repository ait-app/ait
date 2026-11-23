#!/usr/bin/env node
// Loopback OpenAI-compatible model fixture for official-OMP native export E2E.
// Serves /v1/chat/completions with a deterministic SSE reply; no external network.
// Usage: node omp-loopback-model.mjs <port-file>
import http from "node:http";
import { writeFileSync } from "node:fs";

const portFile = process.argv[2];
let heldResponse = null;
const server = http.createServer((req, res) => {
  if (req.method === "GET" && req.url === "/held") {
    res.writeHead(200, { "content-type": "application/json" });
    res.end(JSON.stringify({ active: heldResponse !== null }));
    return;
  }
  if (req.method === "POST" && req.url === "/release") {
    heldResponse?.();
    res.writeHead(204);
    res.end();
    return;
  }
  if (req.method === "POST" && req.url === "/v1/chat/completions") {
    let body = "";
    req.on("data", (chunk) => (body += chunk));
    req.on("end", () => {
      const delta = { role: "assistant", content: "Native export loopback reply." };
      const first = {
        id: "fixture",
        object: "chat.completion.chunk",
        created: 0,
        model: "fixture",
        choices: [{ index: 0, delta, finish_reason: null }],
      };
      const last = {
        id: "fixture",
        object: "chat.completion.chunk",
        created: 0,
        model: "fixture",
        choices: [{ index: 0, delta: {}, finish_reason: "stop" }],
        usage: { prompt_tokens: 5, completion_tokens: 2, total_tokens: 7 },
      };
      res.writeHead(200, { "content-type": "text/event-stream" });
      res.write(`data: ${JSON.stringify(first)}\n\n`);
      const finish = () => {
        heldResponse = null;
        res.end(`data: ${JSON.stringify(last)}\n\ndata: [DONE]\n\n`);
      };
      const request = JSON.parse(body);
      const lastMessage = request.messages.at(-1);
      if (JSON.stringify(lastMessage?.content).includes("Hold this turn open while exporting.")) {
        heldResponse = finish;
        res.on("close", () => {
          heldResponse = null;
        });
      } else {
        finish();
      }
    });
    return;
  }
  res.writeHead(404, { "content-type": "application/json" });
  res.end("{}");
});

server.listen(0, "127.0.0.1", () => {
  const { port } = server.address();
  if (portFile) writeFileSync(portFile, String(port));
  console.log(`omp-loopback-model listening on 127.0.0.1:${port}`);
});
