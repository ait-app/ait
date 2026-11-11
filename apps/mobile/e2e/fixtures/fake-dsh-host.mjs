// Loopback DSH native Host fixture for read-only preview e2e.
// Implements the public non-activating endpoints (session/list, session/projections,
// session/page) plus the writer endpoints, and records EVERY request so the test can
// audit that browsing inactive history never creates, follows or prompts a session.
import { createServer } from "node:http";
import { appendFileSync, mkdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { WebSocketServer } from "ws";

// Daemon diagnostics probe the launcher with --version before any Host boot.
if (process.argv.includes("--version")) {
  process.stdout.write("dsh 0.2.2-rc.1 (e2e fixture)\n");
  process.exit(0);
}

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

const SESSION_ID = "e2e-native-session";
const RUNNING_SESSION_ID = "e2e-running-session";
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

// Persisted fixture history can grow between separate read-only Host launches.
const records = turnRecords(Number(readFileSync(statePath, "utf8")));
// A second native session with an unfinished turn: the current writer control that
// read-only browsing must leave untouched (active stays true, journal never changes).
const runningRecords = (() => {
  const settled = turnRecords(1);
  const seq = settled.length;
  settled.push({
    type: "event",
    event: {
      seq,
      time: BASE_TIME + seq,
      type: "user/message",
      data: {
        id: `user-${seq}`,
        source: { kind: "user" },
        content: [{ type: "text", text: "running question" }],
      },
    },
  });
  settled.push({
    type: "event",
    event: { seq: seq + 1, type: "turn/start", time: BASE_TIME + seq + 1, data: { turn: 1 } },
  });
  // Keep turn/start outside the newest 16-record page, as in a long unfinished turn.
  for (let index = 0; index < 20; index += 1) {
    const messageSeq = settled.length;
    settled.push({
      type: "event",
      event: {
        seq: messageSeq,
        time: BASE_TIME + messageSeq,
        type: "assistant/message",
        data: {
          turn: 1,
          message: {
            id: `running-progress-${index}`,
            content: [{ type: "text", text: `running progress ${index}` }],
          },
        },
      },
    });
  }
  return settled;
})();

function audit(entry) {
  appendFileSync(auditPath, `${JSON.stringify(entry)}\n`);
}

function page(before, journal = records) {
  const end = journal.findIndex((record) => record.event.seq >= before);
  const stop = end === -1 ? journal.length : end;
  // Keep multiple native pages while allowing a 50-item UI page within the 16-page read budget.
  const start = Math.max(0, stop - 16);
  return { records: journal.slice(start, stop), hasMore: start > 0 };
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
      {
        sessionId: RUNNING_SESSION_ID,
        cwd,
        updatedAt: BASE_TIME + runningRecords.length,
        blank: false,
        running: true,
        agentAvailable: true,
        projections: {
          kind: "cached",
          asOfSeq: runningRecords.length - 1,
          values: { title: "E2E running conversation" },
        },
      },
    ],
  };
}

function projectionsValue(sessionId = SESSION_ID) {
  const journal = sessionId === RUNNING_SESSION_ID ? runningRecords : records;
  return {
    kind: "sequenced",
    asOfSeq: journal.length - 1,
    values: {
      title:
        sessionId === RUNNING_SESSION_ID ? "E2E running conversation" : "E2E native conversation",
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
      return projectionsValue(args.request?.sessionId ?? args.sessionId ?? SESSION_ID);
    case "session/page": {
      const request = args.request ?? {};
      const sessionId = request.address?.sessionId ?? SESSION_ID;
      const journal = sessionId === RUNNING_SESSION_ID ? runningRecords : records;
      const before =
        request.beforeSeq !== undefined ? request.beforeSeq : (request.throughSeq ?? -1) + 1;
      return page(before, journal);
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
      const followArgs = request.payload?.args ?? {};
      const sessionId =
        followArgs.request?.address?.sessionId ?? followArgs.address?.sessionId ?? SESSION_ID;
      const journal = sessionId === RUNNING_SESSION_ID ? runningRecords : records;
      const cut = journal.length - 1;
      const first = page(cut + 1, journal);
      value = {
        type: "snapshot",
        header: { id: sessionId, cwd, createdAt: BASE_TIME },
        cursor: cut,
        records: first.records,
        hasMore: first.hasMore,
        projections: { values: projectionsValue(sessionId).values },
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
  process.stdout.write(`dsh web: http://127.0.0.1:${port}/?token=e2e\n`);
});

setTimeout(
  () => {
    server.close();
    process.exit(0);
  },
  10 * 60 * 1000,
).unref();
