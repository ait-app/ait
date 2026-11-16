import { describe, expect, it } from "vitest";
import type { Agent } from "@/stores/session-store";
import {
  buildTaskBoard,
  filterTaskBoard,
  taskDescendants,
  taskActions,
  taskKey,
  taskLane,
  type TaskBoardHost,
} from "./model";

function agent(id: string, patch: Partial<Agent> = {}): Agent {
  return {
    id,
    serverId: "local",
    provider: "codex",
    status: "idle",
    turn: { phase: "idle", cancellationRequestId: null },
    createdAt: new Date(0),
    updatedAt: new Date(0),
    lastActivityAt: new Date(0),
    lastUserMessageAt: null,
    capabilities: {
      supportsStreaming: true,
      supportsSessionPersistence: true,
      supportsDynamicModes: true,
      supportsMcpServers: true,
      supportsReasoningStream: true,
      supportsToolInvocations: true,
    },
    currentModeId: null,
    availableModes: [],
    pendingPermissions: [],
    persistence: null,
    title: id,
    cwd: "/code/ait",
    model: "gpt-5",
    requiresAttention: false,
    attentionReason: null,
    archivedAt: null,
    parentAgentId: null,
    labels: {},
    ...patch,
  };
}

function host(agents: Agent[], serverId = "local"): TaskBoardHost {
  return {
    serverId,
    label: serverId,
    agents: new Map(agents.map((item) => [item.id, { ...item, serverId }])),
    workspaces: new Map(),
  };
}

describe("task board", () => {
  it("keeps observed children read only and unknown outcomes out of the completed lane", () => {
    const child = agent("observed", {
      parentAgentId: "parent",
      management: "observed",
      observation: {
        rootAgentId: "parent",
        nativeId: "native-child",
        status: "unknown",
        health: "stale",
        observedAt: new Date(0).toISOString(),
        description: null,
        subtitle: null,
      },
    });
    const model = buildTaskBoard([host([agent("parent"), child])]);
    const task = model.byKey.get(taskKey("local", "observed"))!;
    expect(model.roots).toHaveLength(1);
    expect(taskLane(task.ownBucket)).toBe("review");
    expect(taskActions(task)).toEqual({ approve: false, stop: false, read: false, archive: false });
    expect(taskActions(model.roots[0]).archive).toBe(false);
    task.agent.observation!.status = "completed";
    task.agent.capabilities.supportsArchive = true;
    expect(taskActions(task).archive).toBe(true);
  });

  it("honors capability restrictions on internal sessions and active managed tasks", () => {
    const model = buildTaskBoard([
      host([
        agent("running", { status: "running" }),
        agent("inspection", {
          status: "running",
          capabilities: {
            ...agent("base").capabilities,
            supportsMessageSend: false,
            supportsCancel: false,
            supportsArchive: false,
          },
        }),
      ]),
    ]);
    expect(taskActions(model.byKey.get(taskKey("local", "running"))!)).toEqual({
      approve: true,
      stop: true,
      read: false,
      archive: false,
    });
    expect(taskActions(model.byKey.get(taskKey("local", "inspection"))!)).toEqual({
      approve: false,
      stop: false,
      read: false,
      archive: false,
    });
  });

  it.each([
    ["running", "running", false],
    ["failed", "failed", true],
    ["completed", "done", true],
    ["canceled", "done", true],
    ["unknown", "attention", false],
  ] as const)(
    "preserves the last observed %s result after the observation channel closes",
    (status, bucket, canArchive) => {
      const child = agent("observed", {
        parentAgentId: "parent",
        status: "closed",
        management: "observed",
        capabilities: { ...agent("base").capabilities, supportsArchive: true },
        observation: {
          rootAgentId: "parent",
          nativeId: "native-child",
          status,
          health: "stale",
          observedAt: new Date(0).toISOString(),
          description: null,
          subtitle: null,
        },
      });
      const model = buildTaskBoard([host([agent("parent"), child])]);
      const task = model.byKey.get(taskKey("local", "observed"))!;
      expect(task.ownBucket).toBe(bucket);
      expect(model.roots[0].bucket).toBe(bucket);
      expect(taskActions(task).archive).toBe(canArchive);
      expect(taskActions(model.roots[0]).archive).toBe(canArchive);
    },
  );

  it("bubbles child input and activity to the parent without duplicating child cards", () => {
    const model = buildTaskBoard([
      host([
        agent("parent"),
        agent("child", { parentAgentId: "parent", status: "running" }),
        agent("grandchild", {
          parentAgentId: "child",
          attentionReason: "permission",
          lastActivityAt: new Date(500),
        }),
      ]),
    ]);
    expect(model.roots).toHaveLength(1);
    expect(model.roots[0].bucket).toBe("needs_input");
    expect(model.roots[0].ownBucket).toBe("done");
    expect(model.roots[0].activityAt).toBe(500);
    expect(taskDescendants(model.roots[0]).map((item) => item.agent.id)).toEqual([
      "child",
      "grandchild",
    ]);
    expect(taskLane(model.roots[0].bucket)).toBe("needs_action");
  });

  it("keeps active children above completed parents and recognizes an open turn", () => {
    const model = buildTaskBoard([
      host([
        agent("parent", { requiresAttention: true, attentionReason: "finished" }),
        agent("child", {
          parentAgentId: "parent",
          turn: { phase: "open", startedAt: new Date(0), turnId: "t", cancellationRequestId: null },
        }),
      ]),
    ]);
    expect(model.roots[0].bucket).toBe("running");
    expect(taskLane(model.roots[0].ownBucket)).toBe("review");
  });

  it("scopes identity and ancestry to a host and retains orphan tasks", () => {
    const model = buildTaskBoard([
      host([agent("same")]),
      host(
        [
          agent("same"),
          agent("child", { parentAgentId: "same" }),
          agent("orphan", { parentAgentId: "missing" }),
        ],
        "remote",
      ),
    ]);
    expect(model.byKey.size).toBe(4);
    expect(model.byKey.get(taskKey("local", "same"))?.children).toHaveLength(0);
    expect(model.byKey.get(taskKey("remote", "same"))?.children).toHaveLength(1);
    expect(model.roots.map((item) => item.key)).toContain(taskKey("remote", "orphan"));
  });

  it("does not lose or infinitely traverse cyclic ancestry", () => {
    const model = buildTaskBoard([
      host([
        agent("a", { parentAgentId: "b" }),
        agent("b", { parentAgentId: "a" }),
        agent("self", { parentAgentId: "self" }),
        agent("c", { parentAgentId: "a" }),
      ]),
    ]);
    const visible = model.roots.flatMap((item) => [item, ...taskDescendants(item)]);
    expect(new Set(visible.map((item) => item.key)).size).toBe(4);
    expect(visible).toHaveLength(4);
  });

  it("hides archived agents while retaining their active children", () => {
    const model = buildTaskBoard([
      host([
        agent("archived", { archivedAt: new Date() }),
        agent("child", { parentAgentId: "archived" }),
      ]),
    ]);
    expect(model.roots.map((item) => item.agent.id)).toEqual(["child"]);
  });

  it("searches descendant titles and combines project/host filters", () => {
    const model = buildTaskBoard([
      host([agent("main"), agent("child", { parentAgentId: "main", title: "Fix login callback" })]),
      host([agent("other", { cwd: "/other/project" })], "remote"),
    ]);
    const filters = {
      hostId: "local",
      projectKey: taskKey("local", "/code/ait"),
      search: "LOGIN gpt-5",
      sort: "recent" as const,
    };
    expect(filterTaskBoard(model.roots, filters).map((item) => item.agent.id)).toEqual(["main"]);
    expect(filterTaskBoard(model.roots, { ...filters, hostId: "remote" })).toEqual([]);
  });

  it("prioritizes approvals over failures and sorts within other lanes", () => {
    const model = buildTaskBoard([
      host([
        agent("failed", { status: "error", lastActivityAt: new Date(200) }),
        agent("input", { attentionReason: "permission" }),
        agent("old"),
        agent("new", { lastActivityAt: new Date(100) }),
      ]),
    ]);
    const filters = { hostId: null, projectKey: null, search: "", sort: "recent" as const };
    const result = filterTaskBoard(model.roots, filters);
    expect(
      result
        .filter((item) => taskLane(item.bucket) === "needs_action")
        .map((item) => item.agent.id),
    ).toEqual(["input"]);
    expect(
      result.filter((item) => taskLane(item.bucket) === "review").map((item) => item.agent.id),
    ).toEqual(["failed"]);
    expect(
      result.filter((item) => taskLane(item.bucket) === "done").map((item) => item.agent.id),
    ).toEqual(["new", "old"]);
    expect(
      filterTaskBoard(model.roots, { ...filters, sort: "oldest" })
        .filter((item) => taskLane(item.bucket) === "done")
        .map((item) => item.agent.id),
    ).toEqual(["old", "new"]);
  });

  it("searches the displayed runtime model of roots and descendants using provider defaults", () => {
    const runtimeInfo = { provider: "codex" as const, sessionId: null, model: "default-model" };
    const model = buildTaskBoard([
      host([
        agent("root", { model: null, runtimeInfo }),
        agent("parent"),
        agent("child", { parentAgentId: "parent", model: null, runtimeInfo }),
        agent("explicit", { model: "selected-model", runtimeInfo }),
        agent("unresolved", { model: null }),
      ]),
    ]);
    const filters = {
      hostId: null,
      projectKey: null,
      search: "DEFAULT-MODEL",
      sort: "recent" as const,
    };
    expect(
      filterTaskBoard(model.roots, filters)
        .map((task) => task.agent.id)
        .sort(),
    ).toEqual(["parent", "root"]);
    expect(
      filterTaskBoard(model.roots, { ...filters, search: "selected-model" }).map(
        (task) => task.agent.id,
      ),
    ).toEqual(["explicit"]);
  });
});
