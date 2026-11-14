// @vitest-environment jsdom
import type { PropsWithChildren } from "react";
import { act, cleanup, renderHook } from "@testing-library/react";
import {
  defaultScheduler,
  notifyManager,
  QueryClient,
  QueryClientProvider,
} from "@tanstack/react-query";
import type { ProviderSnapshotEntry } from "@ait/protocol/agent-types";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { providersSnapshotQueryKey } from "@/data/providers-snapshot";
import { useProvidersSnapshot } from "./use-providers-snapshot";

const host = vi.hoisted(() => ({
  connected: true,
  active: true,
  connectionEpoch: 1,
  clientGeneration: 1,
  client: { getProvidersSnapshot: vi.fn(), refreshProvidersSnapshot: vi.fn() },
  listeners: new Set<() => void>(),
}));

vi.mock("@/runtime/host-runtime", () => ({
  getHostRuntimeStore: () => ({
    getSnapshot: () => ({
      client: host.client,
      connectionStatus: host.connected ? "online" : "offline",
      connectionEpoch: host.connectionEpoch,
      clientGeneration: host.clientGeneration,
    }),
    subscribe: (_serverId: string, notify: () => void) => {
      host.listeners.add(notify);
      return () => host.listeners.delete(notify);
    },
  }),
  useHostRuntimeClient: () => host.client,
  useHostRuntimeIsConnected: () => host.connected,
}));
vi.mock("@/components/retained-panel", () => ({
  useRetainedPanelActive: () => host.active,
}));
vi.mock("@/stores/session-store", () => ({ useSessionStore: () => true }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("@react-native-async-storage/async-storage", () => ({
  default: {
    getItem: async () => null,
    getAllKeys: async () => [],
    multiGet: async () => [],
    setItem: async () => {},
    removeItem: async () => {},
    multiRemove: async () => {},
  },
}));

const serverId = "discovery-host";
const cwd = "/repo";
let queryClient: QueryClient;
const originalClient = host.client;

function wrapper({ children }: PropsWithChildren) {
  return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
}

function snapshot(status: ProviderSnapshotEntry["status"], revision = 1) {
  return {
    requestId: "discovery",
    cwd,
    snapshotHash: `${status}-${revision}`,
    generation: "daemon",
    revision,
    refreshing: status === "loading" ? ["codex"] : [],
    generatedAt: "2026-10-26T00:00:00.000Z",
    entries: [
      {
        provider: "codex",
        status,
        enabled: true,
        models: status === "ready" ? [{ provider: "codex", id: "test", label: "Test" }] : [],
      },
    ],
  };
}

describe("provider discovery without completion pushes", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    // Flush observer notifications with React's async act instead of a second timer tick.
    notifyManager.setScheduler((callback) => void Promise.resolve().then(callback));
    host.connected = true;
    host.active = true;
    host.connectionEpoch = 1;
    host.clientGeneration = 1;
    host.client = originalClient;
    host.listeners.clear();
    host.client.refreshProvidersSnapshot.mockReset();
    host.client.getProvidersSnapshot.mockReset();
    queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  });

  afterEach(() => {
    cleanup();
    queryClient.clear();
    notifyManager.setScheduler(defaultScheduler);
    vi.useRealTimers();
  });

  it.each(["ready", "error", "unavailable"] as const)(
    "rechecks loading until %s and then stops",
    async (status) => {
      const terminal = snapshot(status, 2);
      host.client.getProvidersSnapshot
        .mockResolvedValueOnce(snapshot("loading"))
        .mockResolvedValue(terminal);
      const { result } = renderHook(() => useProvidersSnapshot(serverId, { cwd }), { wrapper });

      await act(() => vi.advanceTimersByTimeAsync(0));
      expect(result.current.entries?.[0]?.status).toBe("loading");
      await act(() => vi.advanceTimersByTimeAsync(1_000));

      expect(result.current.entries).toEqual(terminal.entries);
      expect(host.client.getProvidersSnapshot).toHaveBeenCalledTimes(2);
      await act(() => vi.advanceTimersByTimeAsync(60_000));
      expect(host.client.getProvidersSnapshot).toHaveBeenCalledTimes(2);
    },
  );

  it("resumes discovery from a cached loading snapshot when the panel opens", async () => {
    queryClient.setQueryData(providersSnapshotQueryKey(serverId, cwd), snapshot("loading"));
    const terminal = snapshot("ready", 2);
    host.client.getProvidersSnapshot.mockResolvedValue(terminal);
    const { result } = renderHook(() => useProvidersSnapshot(serverId, { cwd }), { wrapper });

    await act(() => vi.advanceTimersByTimeAsync(1_000));

    expect(result.current.entries).toEqual(terminal.entries);
    expect(host.client.getProvidersSnapshot).toHaveBeenCalledTimes(1);
  });

  it("rechecks an in-progress refresh even when cached models are already ready", async () => {
    queryClient.setQueryData(providersSnapshotQueryKey(serverId, cwd), {
      ...snapshot("ready"),
      refreshing: ["codex"],
    });
    host.client.getProvidersSnapshot.mockResolvedValue(snapshot("ready", 2));
    renderHook(() => useProvidersSnapshot(serverId, { cwd }), { wrapper });

    await act(() => vi.advanceTimersByTimeAsync(1_000));

    expect(host.client.getProvidersSnapshot).toHaveBeenCalledTimes(1);
    expect(queryClient.getQueryData(providersSnapshotQueryKey(serverId, cwd))).toMatchObject({
      revision: 2,
      refreshing: [],
    });
    await act(() => vi.advanceTimersByTimeAsync(60_000));
    expect(host.client.getProvidersSnapshot).toHaveBeenCalledTimes(1);
  });

  it.each(["enabled", "connected", "active"] as const)(
    "pauses rechecks while %s is false and resumes afterward",
    async (flag) => {
      host.client.getProvidersSnapshot
        .mockResolvedValueOnce(snapshot("loading"))
        .mockResolvedValue(snapshot("ready", 2));
      const { result, rerender } = renderHook(
        ({ enabled }) => useProvidersSnapshot(serverId, { cwd, enabled }),
        { wrapper, initialProps: { enabled: true } },
      );
      await act(() => vi.advanceTimersByTimeAsync(0));
      expect(result.current.entries?.[0]?.status).toBe("loading");

      if (flag !== "enabled") host[flag] = false;
      rerender({ enabled: flag !== "enabled" });
      await act(() => vi.advanceTimersByTimeAsync(5_000));
      expect(host.client.getProvidersSnapshot).toHaveBeenCalledTimes(1);

      if (flag !== "enabled") host[flag] = true;
      rerender({ enabled: true });
      await act(() => vi.advanceTimersByTimeAsync(1_000));
      expect(result.current.entries?.[0]?.status).toBe("ready");
      expect(host.client.getProvidersSnapshot).toHaveBeenCalledTimes(2);
    },
  );

  it("stops rechecks when a completion push updates the query cache", async () => {
    host.client.getProvidersSnapshot.mockResolvedValue(snapshot("loading"));
    const { result } = renderHook(() => useProvidersSnapshot(serverId, { cwd }), { wrapper });
    await act(() => vi.advanceTimersByTimeAsync(0));

    act(() => {
      queryClient.setQueryData(providersSnapshotQueryKey(serverId, cwd), snapshot("ready", 2));
    });
    await act(() => vi.advanceTimersByTimeAsync(60_000));

    expect(result.current.entries?.[0]?.status).toBe("ready");
    expect(host.client.getProvidersSnapshot).toHaveBeenCalledTimes(1);
  });

  it("does not publish an old connection's late refresh into the replacement connection", async () => {
    host.client.getProvidersSnapshot.mockResolvedValue(snapshot("ready"));
    const gate = Promise.withResolvers<{ requestId: string; acknowledged: boolean }>();
    host.client.refreshProvidersSnapshot.mockReturnValue(gate.promise);
    const { result, rerender } = renderHook(() => useProvidersSnapshot(serverId, { cwd }), {
      wrapper,
    });
    await act(() => vi.advanceTimersByTimeAsync(0));

    act(() => result.current.refetchIfStale("codex"));
    await act(() => vi.advanceTimersByTimeAsync(0));
    expect(originalClient.refreshProvidersSnapshot).toHaveBeenCalledTimes(1);

    const replacement = {
      getProvidersSnapshot: vi.fn().mockResolvedValue({
        ...snapshot("ready", 2),
        entries: [
          {
            ...snapshot("ready", 2).entries[0],
            models: [{ provider: "codex", id: "new", label: "New" }],
          },
        ],
      }),
      refreshProvidersSnapshot: vi.fn(),
    };
    act(() => {
      host.client = replacement;
      for (const notify of host.listeners) notify();
      rerender();
    });
    await act(() => vi.advanceTimersByTimeAsync(0));
    await act(async () => {
      await queryClient.refetchQueries({
        queryKey: providersSnapshotQueryKey(serverId, cwd),
        exact: true,
      });
    });
    await act(async () => {
      gate.resolve({ requestId: "old", acknowledged: true });
      await gate.promise;
      await vi.advanceTimersByTimeAsync(0);
    });

    expect(originalClient.getProvidersSnapshot).toHaveBeenCalledTimes(1);
    expect(result.current.entries?.[0]?.models?.[0]?.id).toBe("new");
    expect(result.current.error).toBeNull();
  });

  it("rejects a late refresh after the same client reconnects with a new epoch", async () => {
    host.client.getProvidersSnapshot.mockResolvedValue(snapshot("ready"));
    const gate = Promise.withResolvers<{ requestId: string; acknowledged: boolean }>();
    host.client.refreshProvidersSnapshot.mockReturnValue(gate.promise);
    const { result } = renderHook(() => useProvidersSnapshot(serverId, { cwd }), { wrapper });
    await act(() => vi.advanceTimersByTimeAsync(0));
    act(() => result.current.refetchIfStale("codex"));
    await act(() => vi.advanceTimersByTimeAsync(0));
    expect(host.client.refreshProvidersSnapshot).toHaveBeenCalledTimes(1);

    act(() => {
      host.connectionEpoch += 1;
      for (const notify of host.listeners) notify();
      queryClient.setQueryData(providersSnapshotQueryKey(serverId, cwd), snapshot("ready", 2));
    });
    await act(async () => {
      gate.resolve({ requestId: "old-epoch", acknowledged: true });
      await gate.promise;
      await vi.advanceTimersByTimeAsync(0);
    });

    expect(host.client.getProvidersSnapshot).toHaveBeenCalledTimes(1);
    expect(queryClient.getQueryData(providersSnapshotQueryKey(serverId, cwd))).toMatchObject({
      revision: 2,
    });
    expect(result.current.error).toBeNull();
  });

  it("does not apply a late refresh after the working directory changes", async () => {
    host.client.getProvidersSnapshot.mockResolvedValue(snapshot("ready"));
    const gate = Promise.withResolvers<{ requestId: string; acknowledged: boolean }>();
    host.client.refreshProvidersSnapshot.mockReturnValue(gate.promise);
    const { result, rerender } = renderHook(
      ({ currentCwd }: { currentCwd: string }) =>
        useProvidersSnapshot(serverId, { cwd: currentCwd }),
      { wrapper, initialProps: { currentCwd: cwd } },
    );
    await act(() => vi.advanceTimersByTimeAsync(0));

    act(() => result.current.refetchIfStale("codex"));
    await act(() => vi.advanceTimersByTimeAsync(0));
    expect(originalClient.refreshProvidersSnapshot).toHaveBeenCalledTimes(1);
    const oldDirectoryFetches = () =>
      originalClient.getProvidersSnapshot.mock.calls.filter(
        ([options]) => (options as { cwd?: string } | undefined)?.cwd !== "/other-repo",
      ).length;
    const fetchesBefore = oldDirectoryFetches();

    const otherCwd = "/other-repo";
    host.client.getProvidersSnapshot.mockResolvedValue(snapshot("ready", 2));
    rerender({ currentCwd: otherCwd });
    await act(() => vi.advanceTimersByTimeAsync(0));
    await act(async () => {
      gate.resolve({ requestId: "old-directory", acknowledged: true });
      await gate.promise;
      await vi.advanceTimersByTimeAsync(0);
    });

    // The settled refresh belongs to the previous directory: it must not fetch
    // that directory again or publish into the current one.
    expect(oldDirectoryFetches()).toBe(fetchesBefore);
    expect(queryClient.getQueryData(providersSnapshotQueryKey(serverId, otherCwd))).toMatchObject({
      revision: 2,
    });
    expect(result.current.error).toBeNull();
  });
});
