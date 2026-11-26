import { expect, it } from "vitest";
import type { SubagentRow } from "@/subagents/select";
import { findVibeWorkerRow, readVibe, vibeWorkersForDisplay } from "./vibe-model";

it("requires a confirmed enabled flag and distinguishes absent data from an empty registry", () => {
  for (const raw of [undefined, null, {}, { enabled: false }, { enabled: "true" }])
    expect(readVibe(raw)).toBeNull();
  for (const workers of [undefined, null, {}, [null], [{ id: "" }]])
    expect(readVibe({ enabled: true, workers })).toEqual({ workers: null });
  expect(readVibe({ enabled: true, workers: [] })).toEqual({ workers: [] });
});

it("preserves task and model facts without promoting an unrecognized state to running", () => {
  expect(
    readVibe({
      enabled: true,
      workers: [
        {
          id: "native-1",
          description: "检查布局",
          model: '["google", "gemini-4-carbon"]',
          status: "running",
        },
        { id: "native-2", model: "custom:model", status: "new-state" },
      ],
    }),
  ).toEqual({
    workers: [
      {
        id: "native-1",
        description: "检查布局",
        model: "google / gemini-4-carbon",
        status: "running",
      },
      { id: "native-2", description: "", model: "custom:model", status: "unknown" },
    ],
  });
});

it("does not show a false task count for a duplicate native identity", () => {
  expect(readVibe({ enabled: true, workers: [{ id: "same" }, { id: "same" }] })).toEqual({
    workers: null,
  });
});

const observed = (id: string, serverId: string, rootAgentId: string): SubagentRow => ({
  kind: "paseo",
  id,
  serverId,
  provider: "omp",
  title: "检查布局",
  description: "检查布局",
  subtitle: null,
  status: "running",
  turn: { phase: "open", turnId: "turn", startedAt: new Date(0), cancellationRequestId: null },
  requiresAttention: false,
  createdAt: new Date(0),
  observation: {
    nativeId: "native-1",
    rootAgentId,
    status: "running",
    health: "current",
    observedAt: new Date(0).toISOString(),
    description: "检查布局",
    subtitle: null,
  },
});

it("opens the unified child only on the owning host and parent, never a colliding native id", () => {
  const remote = observed("remote", "another-host", "parent");
  const foreign = observed("foreign", "host", "another-parent");
  const child = observed("ait-child", "host", "parent");
  expect(findVibeWorkerRow("native-1", [remote, foreign], "host", "parent")).toBeUndefined();
  expect(findVibeWorkerRow("native-1", [remote, foreign, child], "host", "parent")).toBe(child);
  expect(findVibeWorkerRow("unregistered", [child], "host", "parent")).toBeUndefined();
});

it("resolves legacy provider rows without treating native ids as Ait ids", () => {
  const row: SubagentRow = {
    kind: "provider",
    serverId: "host",
    parentAgentId: "parent",
    id: "native-1",
    provider: "omp",
    title: "检查布局",
    description: "检查布局",
    subtitle: null,
    status: "running",
    requiresAttention: false,
    createdAt: new Date(0),
  };
  expect(findVibeWorkerRow("native-1", [row], "host", "parent")).toBe(row);
  expect(findVibeWorkerRow("native-1", [row], "host", "elsewhere")).toBeUndefined();
});

it("does not expose truncated JSON model routes as model names", () => {
  expect(
    readVibe({ enabled: true, workers: [{ id: "broken", model: '["google", "truncated' }] })
      ?.workers?.[0]?.model,
  ).toBe("");
});

it.each(["stale", "unavailable", "current"] as const)(
  "keeps renamed tasks and marks a %s observation honestly",
  (health) => {
    const child = observed("ait-child", "host", "parent");
    if (child.kind !== "paseo") throw new Error("Expected observed child");
    child.title = "用户重命名的任务";
    child.observation = { ...child.observation!, health };
    const snapshot = readVibe({
      enabled: true,
      workers: [{ id: "native-1", description: "旧任务名", status: "running" }],
    })!;
    expect(vibeWorkersForDisplay(snapshot, [child], "host", "parent").workers?.[0]).toMatchObject({
      description: "用户重命名的任务",
      status: health === "current" ? "running" : "unknown",
    });
    expect(snapshot.workers?.[0]?.status).toBe("running");
  },
);

it("orders registered workers by creation time and leaves undiscovered workers visible", () => {
  const first = observed("first", "host", "parent");
  const second = observed("second", "host", "parent");
  if (second.kind !== "paseo") throw new Error("Expected observed child");
  second.createdAt = new Date(1000);
  second.observation = { ...second.observation!, nativeId: "native-2" };
  const snapshot = readVibe({
    enabled: true,
    workers: [{ id: "native-2" }, { id: "undiscovered" }, { id: "native-1" }],
  })!;
  expect(
    vibeWorkersForDisplay(snapshot, [second, first], "host", "parent").workers?.map((w) => w.id),
  ).toEqual(["native-1", "native-2", "undiscovered"]);
});

it("retains recorded terminal results and task names when an observed title is still a placeholder", () => {
  const child = observed("ait-child", "host", "parent");
  if (child.kind !== "paseo") throw new Error("Expected observed child");
  child.title = "New agent";
  child.observation = { ...child.observation!, health: "stale" };
  const snapshot = readVibe({
    enabled: true,
    workers: [{ id: "native-1", description: "检查布局", status: "completed" }],
  })!;
  expect(vibeWorkersForDisplay(snapshot, [child], "host", "parent").workers?.[0]).toMatchObject({
    description: "检查布局",
    status: "completed",
  });
});
