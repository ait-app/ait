// @vitest-environment jsdom
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useTaskBoard } from "./use-task-board";

const state = vi.hoisted(() => ({
  hosts: [{ serverId: "local", label: "Local" }],
  version: 0,
  listeners: new Set<() => void>(),
  snapshot: {
    connectionStatus: "online" as const,
    agentDirectoryStatus: "ready" as const,
    agentDirectoryError: null,
    workspaceDirectoryError: null as string | null,
  },
  refreshDirectories: vi.fn(),
  retryConnection: vi.fn(),
}));

const runtime = {
  getSnapshot: () => state.snapshot,
  getVersion: () => state.version,
  subscribeAll: (listener: () => void) => {
    state.listeners.add(listener);
    return () => {
      state.listeners.delete(listener);
    };
  },
  acquireDirectoryDemand: () => () => {},
  refreshDirectories: state.refreshDirectories,
  retryConnection: state.retryConnection,
};

vi.mock("@/runtime/host-runtime", () => ({
  useHosts: () => state.hosts,
  useHostRuntimeConnectionStatuses: () => new Map([["local", state.snapshot.connectionStatus]]),
  getHostRuntimeStore: () => runtime,
}));

afterEach(cleanup);

it("reports and retries workspace-directory failures while the agent directory stays online", () => {
  const { result } = renderHook(useTaskBoard);
  expect(result.current.unavailableHosts).toEqual([]);
  const roots = result.current.roots;

  act(() => {
    state.snapshot = { ...state.snapshot, workspaceDirectoryError: "Workspace directory failed" };
    state.version += 1;
    state.listeners.forEach((listener) => listener());
  });
  expect(result.current.unavailableHosts).toEqual(state.hosts);
  expect(result.current.allUnavailable).toBe(true);
  expect(result.current.isInitialLoad).toBe(false);
  expect(result.current.roots).toBe(roots);

  act(() => result.current.refresh());
  expect(state.refreshDirectories).toHaveBeenCalledExactlyOnceWith("local");
  expect(state.retryConnection).not.toHaveBeenCalled();

  act(() => {
    state.snapshot = { ...state.snapshot, workspaceDirectoryError: null };
    state.version += 1;
    state.listeners.forEach((listener) => listener());
  });
  expect(result.current.unavailableHosts).toEqual([]);
  expect(result.current.allUnavailable).toBe(false);
});
