import { expect, it, vi } from "vitest";
import { collectAllTabs, createWorkspaceLayoutStore } from "./workspace-layout-store";

vi.mock("@react-native-async-storage/async-storage", () => ({
  default: { getItem: async () => null, setItem: async () => {}, removeItem: async () => {} },
}));

function windows() {
  const entries = new Map<string, string>();
  const storage = {
    getItem: (key: string) => entries.get(key) ?? null,
    setItem: (key: string, value: string) => {
      entries.set(key, value);
    },
    removeItem: (key: string) => {
      entries.delete(key);
    },
  };
  return () => createWorkspaceLayoutStore(undefined, storage);
}

it("restores changes saved by two windows in different workspaces", async () => {
  const createWindow = windows();
  const first = createWindow();
  const second = createWindow();
  await Promise.all([first.persist.rehydrate(), second.persist.rehydrate()]);
  first.getState().openTab({
    workspaceKey: "host:a",
    target: { kind: "agent", agentId: "agent-a" },
    intent: "reveal",
  });
  second.getState().openTab({
    workspaceKey: "host:b",
    target: { kind: "agent", agentId: "agent-b" },
    intent: "reveal",
  });
  const reopened = createWindow();
  await reopened.persist.rehydrate();
  expect(Object.keys(reopened.getState().layoutByWorkspace).sort()).toEqual(["host:a", "host:b"]);
  for (const id of ["a", "b"]) {
    expect(collectAllTabs(reopened.getState().layoutByWorkspace[`host:${id}`].root)).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ target: { kind: "agent", agentId: `agent-${id}` } }),
      ]),
    );
  }
});

it("keeps live tabs independent when two windows view the same workspace", async () => {
  const createWindow = windows();
  const first = createWindow();
  const second = createWindow();
  await Promise.all([first.persist.rehydrate(), second.persist.rehydrate()]);
  first.getState().openTab({
    workspaceKey: "host:a",
    target: { kind: "agent", agentId: "first" },
    intent: "reveal",
  });
  second.getState().openTab({
    workspaceKey: "host:a",
    target: { kind: "agent", agentId: "second" },
    intent: "reveal",
  });
  const agentIds = (store: typeof first) =>
    collectAllTabs(store.getState().layoutByWorkspace["host:a"].root).flatMap((tab) =>
      tab.target.kind === "agent" ? [tab.target.agentId] : [],
    );
  expect(agentIds(first)).toEqual(["first"]);
  expect(agentIds(second)).toEqual(["second"]);
  const reopened = createWindow();
  await reopened.persist.rehydrate();
  expect(agentIds(reopened)).toEqual(["second"]);
  expect(agentIds(first)).toEqual(["first"]);
});
