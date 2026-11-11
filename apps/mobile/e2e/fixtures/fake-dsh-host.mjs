// Loopback DSH native Host fixture for read-only preview e2e.
// Implements the public non-activating endpoints (session/list, session/projections,
// session/page) plus the writer endpoints, and records EVERY request so the test can
// audit that browsing inactive history never creates, follows or prompts a session.
import { createServer } from "node:http";
import { appendFileSync, mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { WebSocketServer } from "ws";

// The daemon launches the Host with current_dir set to the session cwd; mirror that
// identity instead of a configured path so list/page/projections agree with the caller.
const cwd = process.cwd();
const auditPath = process.env.DSH_E2E_AUDIT;
const statePath = process.env.DSH_E2E_STATE;
if (!auditPath || !statePath) {
  throw new Error("DSH_E2E_AUDIT and DSH_E2E_STATE are required");
}
mkdirSync(path.dirname(auditPath), { recursive: true });
// Append across Host launches: one audit trail per test, never reset by a new Host.
writeFileSync(statePath, "");

const SESSION_ID = "e2e-native-session";
const BASE_TIME = 1_760_000_000_000;

function turnRecords(turns) {
  const records = [];
  for (let turn = 0; turn < turns; turn += 1) {
    const events = [
      [
        "user/message",
        {
          id: `user-${turn}`,
          source: { kind: "user" },
          content: [{ type: "text", text: `question ${turn}` }],
        },
      ],
      ["turn/start", { turn }],
      [
        "assistant/message",
        {
          turn,
          message: { id: `answer-${turn}`, content: [{ type: "text", text: `answer ${turn}` }] },
        },
      ],
      ["turn/end", { turn, reason: { kind: "completed" } }],
    ];
    for (const [type, data] of events) {
      const seq = records.length;
      records.push({ type: "event", event: { seq, type, time: BASE_TIME + seq, data } });
    }
  }
  return records;
}

let records = turnRecords(6);

function audit(entry) {
  appendFileSync(auditPath, `${JSON.stringify(entry)}\n`);
}

function page(before) {
  const end = records.findIndex((record) => record.event.seq >= before);
  const stop = end === -1 ? records.length : end;
  const start = Math.max(0, stop - 2);
  return { records: records.slice(start, stop), hasMore: start > 0 };
}

function listValue() {
  return {
    items: [
      {
        sessionId: SESSION_ID,
        cwd,
        updatedAt: BASE_TIME + records.length,
        blank: false,
        running: false,
        agentAvailable: false,
        projections: {
          kind: "cached",
          asOfSeq: records.length - 1,
          values: { title: "E2E native conversation" },
        },
      },
    ],
  };
}

function projectionsValue() {
  return {
    kind: "sequenced",
    asOfSeq: records.length - 1,
    values: {
      title: "E2E native conversation",
      permissions: { currentValue: "workspace-write" },
      modelSelection: { next: null },
    },
  };
}

function handleRpc(method, args) {
  switch (method) {
    case "session/list":
      return listValue();
    case "session/projections":
      return projectionsValue();
    case "session/page": {
      const request = args.request ?? {};
      const before =
        request.beforeSeq !== undefined ? request.beforeSeq : (request.throughSeq ?? -1) + 1;
      return page(before);
    }
    case "session/modelCatalog":
      return {
        default: { provider: "local", model: "test" },
        groups: [{ id: "local", name: "Local", models: [{ id: "test", name: "Test" }] }],
      };
    case "agentPresets/list":
      return { presets: [] };
    case "settings/describe":
      return { namespaces: [] };
    case "credentials/describe":
      return {};
    case "permissionPresets/catalog":
      return { options: [{ value: "workspace-write", name: "Workspace write" }] };
    case "session/create":
      return { sessionId: SESSION_ID };
    case "session/selectModel":
      return { selected: args.request ?? {} };
    case "session/prompt":
    case "session/cancel":
      return { accepted: true };
    case "commands/execute":
      return { result: { kind: "success" } };
    default:
      return undefined;
  }
}

const server = createServer((req, res) => {
  const url = new URL(req.url ?? "/", "http://127.0.0.1");
  if (req.method === "GET" && url.pathname === "/") {
    audit({ kind: "http", method: "auth" });
    res.writeHead(303, { "set-cookie": "dsh=e2e; HttpOnly; SameSite=Strict", location: "/" });
    res.end();
    return;
  }
  if (req.method === "GET" && url.pathname === "/__audit") {
    res.writeHead(200, { "content-type": "application/json" });
    res.end(JSON.stringify({ records: records.length }));
    return;
  }
  if (req.method === "POST" && url.pathname === "/__grow") {
    // Simulate native history growing after a preview opened; the fixed cut must not drift.
    records = turnRecords(9);
    audit({ kind: "control", method: "grow" });
    res.writeHead(200, { "content-type": "application/json" });
    res.end(JSON.stringify({ records: records.length }));
    return;
  }
  if (req.method === "POST" && url.pathname.startsWith("/api/")) {
    let body = "";
    req.on("data", (chunk) => {
      body += chunk;
    });
    req.on("end", () => {
      const request = JSON.parse(body);
      const method = request.method;
      const args = request.payload?.args ?? {};
      audit({ kind: "rpc", method, args });
      const cookie = req.headers.cookie ?? "";
      if (cookie !== "dsh=e2e") {
        res.writeHead(401);
        res.end();
        return;
      }
      const value = handleRpc(method, args);
      if (value === undefined) {
        res.writeHead(404);
        res.end();
        return;
      }
      res.writeHead(200, { "content-type": "application/json" });
      res.end(
        JSON.stringify({
          type: "server-response",
          rpcId: request.rpcId,
          result: { ok: true, value },
        }),
      );
    });
    return;
  }
  res.writeHead(404);
  res.end();
});

const wss = new WebSocketServer({ server, path: "/api/remote.mux" });
wss.on("connection", (socket, req) => {
  if ((req.headers.cookie ?? "") !== "dsh=e2e") {
    socket.close();
    return;
  }
  socket.on("message", (raw) => {
    const request = JSON.parse(String(raw));
    audit({ kind: "stream", method: request.endpoint, args: request.payload?.args ?? {} });
    if (request.type === "cancel") return;
    let value;
    if (request.endpoint === "$events") {
      value = { type: "ready", clientId: "e2e-client" };
    } else if (request.endpoint === "session/follow") {
      const cut = records.length - 1;
      const first = page(cut + 1);
      value = {
        type: "snapshot",
        header: { id: SESSION_ID, cwd, createdAt: BASE_TIME },
        cursor: cut,
        records: first.records,
        hasMore: first.hasMore,
        projections: { values: projectionsValue().values },
      };
    } else if (request.endpoint === "session/control") {
      value = { type: "baseline", value: { projections: {} } };
    } else {
      socket.close();
      return;
    }
    socket.send(JSON.stringify({ type: "item", streamId: request.streamId, value }));
  });
});

server.listen(0, "127.0.0.1", () => {
  const port = server.address().port;
  writeFileSync(statePath, String(port));
  process.stdout.write(`dsh web: http://127.0.0.1:${port}/?token=e2e\n`);
});

setTimeout(
  () => {
    server.close();
    process.exit(0);
  },
  10 * 60 * 1000,
).unref();
