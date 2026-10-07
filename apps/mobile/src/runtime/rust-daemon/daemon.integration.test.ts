import { spawn, type ChildProcess } from "node:child_process";
import { randomBytes } from "node:crypto";
import { once } from "node:events";
import { mkdtemp, mkdir, rm, symlink } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, it } from "vitest";
import WebSocket from "ws";
import { DaemonClient } from "@ait/client/internal/daemon-client";
import { createWebSocketTransportFactory } from "@ait/client/internal/daemon-client-websocket-transport";
import type { WebSocketLike } from "@ait/client/internal/daemon-client-transport-types";
import { createRustDaemonTransportFactory } from "./transport";
import type { SessionOutboundMessage } from "@ait/protocol/messages";

// Opt in with AIT_TEST_RUST_SERVER=/absolute/path/to/target/debug/daemon.
// The only Provider executable is the repository's offline stdio fixture.
it.runIf(Boolean(process.env.AIT_TEST_RUST_SERVER) && process.platform !== "win32")(
  "preserves Paseo SDK creation, directory, permission, resume and terminal behavior",
  async () => {
    const repository = fileURLToPath(new URL("../../../../../", import.meta.url));
    const root = await mkdtemp(join(tmpdir(), "ait-sdk-creation-"));
    const cwd = join(root, "work");
    const token = randomBytes(32).toString("hex");
    await mkdir(cwd);
    await symlink(
      join(repository, "crates/provider/tests/fixtures/codex_app_server.py"),
      join(root, "codex"),
    );
    const child = spawn(
      resolve(repository, process.env.AIT_TEST_RUST_SERVER!),
      ["--data-dir", join(root, "state"), "--listen", "127.0.0.1:0", "--log-level", "info"],
      {
        cwd: root,
        env: {
          ...process.env,
          AIT_SERVER_TOKEN: token,
          AIT_SERVER_CODEX_BIN: join(root, "codex"),
          AIT_SERVER_CLAUDE_BIN: join(root, "unavailable-claude"),
          AIT_SERVER_SKILLS_HOME: join(root, "skills-home"),
          AIT_SERVER_SKILLS_BUNDLE: join(root, "skills-bundle"),
          AIT_SPEECH_PROVIDER: "disabled",
          CLAUDE_CONFIG_DIR: join(root, "claude-config"),
        },
        stdio: ["ignore", "ignore", "pipe"],
      },
    );
    let client: DaemonClient | undefined;
    try {
      const address = await ready(child);
      const base = createWebSocketTransportFactory(
        (url, options) =>
          new WebSocket(url, options?.protocols, {
            headers: options?.headers,
          }) as unknown as WebSocketLike,
      );
      client = new DaemonClient({
        url: `ws://${address}/v1/ws`,
        clientId: "paseo-creation-smoke",
        authHeader: `Bearer ${token}`,
        reconnect: { enabled: false },
        transportFactory: createRustDaemonTransportFactory(base),
      });
      await client.connect();
      const agentPhases: string[] = [];
      const agent = await client.createAgent({
        idempotencyKey: "sdk-agent",
        config: { provider: "codex", cwd, title: "SDK agent" },
        initialPrompt: "SDK agent creation",
        onEvent: (snapshot) => agentPhases.push(snapshot.phase),
      });
      expect(agent.id).toEqual(expect.any(String));
      expect(agentPhases).toEqual(["accepted", "agent_ready", "prompt_started", "completed"]);
      expect((await client.waitForFinish(agent.id, 10000)).lastMessage).toBe(
        "Echo: SDK agent creation",
      );
      const workspacePhases: string[] = [];
      const workspace = await client.createWorkspace({
        idempotencyKey: "sdk-workspace",
        source: { kind: "directory", path: cwd },
        agent: {
          config: { provider: "codex", cwd, title: "SDK workspace" },
          initialPrompt: "SDK workspace creation",
        },
        onEvent: (snapshot) => workspacePhases.push(snapshot.phase),
      });
      expect(workspace.error).toBeNull();
      expect(workspace.agent?.workspaceId).toBe(workspace.workspace?.id);
      expect(workspacePhases).toEqual([
        "accepted",
        "workspace_ready",
        "agent_ready",
        "prompt_started",
        "completed",
      ]);
      expect((await client.waitForFinish(workspace.agent!.id, 10000)).lastMessage).toBe(
        "Echo: SDK workspace creation",
      );
      await verifyReadInterfaces(client, agent.id, cwd);
      await verifyPermissionAndResume(client, agent.id);
      await verifyTerminalActivity(client, cwd, workspace.workspace!.id);
    } finally {
      await client?.close().catch(() => {});
      await stop(child);
      await rm(root, { recursive: true, force: true });
    }
  },
  30000,
);

async function verifyReadInterfaces(client: DaemonClient, agentId: string, cwd: string) {
  const first = await client.fetchAgents({ page: { limit: 1 } });
  expect(first.entries).toHaveLength(1);
  expect(first.pageInfo.hasMore).toBe(true);
  const second = await client.fetchAgents({
    page: { limit: 1, cursor: first.pageInfo.nextCursor! },
  });
  expect(second.entries[0]!.agent.id).not.toBe(first.entries[0]!.agent.id);
  expect((await client.fetchAgentHistory({ search: "SDK" })).entries.length).toBe(2);
  const timeline = await client.fetchAgentTimeline(agentId, { projection: "canonical" });
  expect(timeline.projection).toBe("projected");
  expect(timeline.entries.length).toBeGreaterThan(0);
  expect(
    (await client.searchAgentTimeline({ agentId, query: "SDK agent" })).locations.length,
  ).toBeGreaterThan(0);
  expect((await client.buildAgentForkContext(agentId)).attachment?.text).toContain(
    "SDK agent creation",
  );
  const features = await client.listProviderFeatures({ provider: "codex", cwd });
  expect(features.error).toBeNull();
  expect(features.features).toEqual([]);
  const workspaces = client.observeWorkspaces({ sync: {} });
  const agents = client.observeAgents({ scope: "active", sync: {} });
  const updates: SessionOutboundMessage[] = [];
  const observer = {
    snapshot: () => {},
    update: (message: SessionOutboundMessage) => updates.push(message),
  };
  workspaces.subscribe(observer);
  agents.subscribe(observer);
  try {
    const snapshot = await workspaces.ready;
    const agentSnapshot = await agents.ready;
    expect(snapshot.entries).toHaveLength(2);
    expect(snapshot.sync?.mode).toBe("snapshot");
    expect(snapshot.subscriptionId).toEqual(expect.any(String));
    const workspaceId = snapshot.entries[0]!.id;
    await client.setWorkspaceTitle(workspaceId, "Live workspace title");
    await client.updateAgent(agentId, { name: "Live agent title" });
    await expect
      .poll(() => updates, { timeout: 5000 })
      .toEqual(
        expect.arrayContaining([
          expect.objectContaining({
            type: "workspace.update",
            payload: expect.objectContaining({
              subscriptionId: snapshot.subscriptionId,
              kind: "upsert",
              workspace: expect.objectContaining({
                id: workspaceId,
                title: "Live workspace title",
              }),
            }),
          }),
          expect.objectContaining({
            type: "agent.update",
            payload: expect.objectContaining({
              subscriptionId: agentSnapshot.subscriptionId,
              kind: "upsert",
              agent: expect.objectContaining({
                id: agentId,
                title: "Live agent title",
              }),
            }),
          }),
        ]),
      );
  } finally {
    await workspaces.release();
    await agents.release();
  }
}

async function verifyPermissionAndResume(client: DaemonClient, agentId: string) {
  const events = client.observeEvents(["agent.permission.request", "agent.permission.resolved"]);
  let permissionId: string | undefined;
  const unsubscribe = client.on("agent.permission.request", (message) => {
    if (message.payload.agentId === agentId) permissionId = message.payload.request.id;
  });
  try {
    await events.ready;
    await client.sendAgentMessage(agentId, "permit-command");
    await expect.poll(() => permissionId, { timeout: 5000 }).toEqual(expect.any(String));
    expect((await client.waitForFinish(agentId, 10000)).status).toBe("permission");
    const resolution = await client.respondToPermissionAndWait(
      agentId,
      permissionId!,
      { behavior: "allow" },
      10000,
    );
    expect(resolution.resolution.behavior).toBe("allow");
    expect((await client.waitForFinish(agentId, 10000)).lastMessage).toBe("Echo: approved");
  } finally {
    unsubscribe();
    await events.release();
  }
  const before = await client.fetchAgent({ agentId });
  if (!before?.agent.persistence) throw new Error("Missing native persistence handle");
  expect((await client.archiveAgent(agentId)).archivedAt).toEqual(expect.any(String));
  const resumed = await client.resumeAgent(before.agent.persistence!, { title: "SDK resumed" });
  expect(resumed.id).toBe(agentId);
  expect(resumed.title).toBe("SDK resumed");
  expect(resumed.archivedAt).toBeNull();
  await client.sendAgentMessage(agentId, "after resume");
  expect((await client.waitForFinish(agentId, 10000)).lastMessage).toBe("Echo: after resume");
}

async function verifyTerminalActivity(client: DaemonClient, cwd: string, workspaceId: string) {
  const events = client.observeEvents(["terminal.attention.required"]);
  const notifications: { terminalId: string; reason: string }[] = [];
  const unsubscribe = client.on("terminal.attention.required", (message) => {
    notifications.push(message.payload);
  });
  let terminalId: string | undefined;
  try {
    await events.ready;
    const created = await client.createTerminal(cwd, "SDK terminal", undefined, {
      workspaceId,
      command: "python3",
      args: [
        "-c",
        `import json, os, time, urllib.request
for state in ("running", "needs-input"):
    report = {"terminalId": os.environ["PASEO_TERMINAL_ID"], "token": os.environ["PASEO_ACTIVITY_TOKEN"], "state": state}
    request = urllib.request.Request(os.environ["PASEO_TERMINAL_ACTIVITY_URL"], data=json.dumps(report).encode(), headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=5) as response:
        assert response.status == 204
print("SDK terminal ready", flush=True)
time.sleep(30)
`,
      ],
    });
    expect(created.error).toBeNull();
    terminalId = created.terminal?.id;
    expect(terminalId).toEqual(expect.any(String));
    await expect
      .poll(() => notifications, { timeout: 5000 })
      .toEqual([expect.objectContaining({ terminalId, reason: "needs_input" })]);
    const listed = await client.listTerminals(undefined, undefined, { workspaceId });
    expect(listed.terminals.find((terminal) => terminal.id === terminalId)?.activity).toMatchObject(
      {
        state: "idle",
        attentionReason: "needs_input",
      },
    );
    await expect
      .poll(async () => (await client.captureTerminal(terminalId!)).lines.join("\n"), {
        timeout: 5000,
      })
      .toContain("SDK terminal ready");
  } finally {
    if (terminalId) expect((await client.killTerminal(terminalId)).success).toBe(true);
    unsubscribe();
    await events.release();
  }
}

function ready(child: ChildProcess): Promise<string> {
  return new Promise((resolve, reject) => {
    let output = "";
    const timer = setTimeout(() => reject(new Error(`Server did not start: ${output}`)), 10000);
    child.once("error", (error) => {
      clearTimeout(timer);
      reject(error);
    });
    child.stderr!.on("data", (chunk) => {
      output = (output + String(chunk)).slice(-16384);
      const match = output.match(/listen=(127\.0\.0\.1:\d+)/);
      if (match) {
        clearTimeout(timer);
        resolve(match[1]!);
      }
    });
  });
}

async function stop(child: ChildProcess) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  const stopped = once(child, "exit");
  child.kill("SIGTERM");
  const timer = setTimeout(() => child.kill("SIGKILL"), 5000);
  try {
    await stopped;
  } finally {
    clearTimeout(timer);
  }
}
