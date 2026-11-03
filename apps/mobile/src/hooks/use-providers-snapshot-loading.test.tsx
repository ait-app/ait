// @vitest-environment jsdom
import type { PropsWithChildren } from "react";
import { act, cleanup, renderHook } from "@testing-library/react";
import {
  defaultScheduler,
  notifyManager,
  QueryClient,
  QueryClientProvider,
} from "@tanstack/react-query";
import type { AgentProvider, ProviderSnapshotEntry } from "@ait/protocol/agent-types";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { providersSnapshotQueryKey, useProvidersSnapshot } from "./use-providers-snapshot";

const host = vi.hoisted(() => ({
  connected: true,
  active: true,
  client: { getProvidersSnapshot: vi.fn(), refreshProvidersSnapshot: vi.fn() },
}));

vi.mock("@/runtime/host-runtime", () => ({
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

function wrapper({ children }: PropsWithChildren) {
  return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
}

function snapshot(
  status: ProviderSnapshotEntry["status"],
  revision = 1,
  provider: AgentProvider = "codex",
) {
  return {
    requestId: "discovery",
    cwd,
    snapshotHash: `${provider}-${status}-${revision}`,
    generation: "daemon",
    revision,
    refreshing: status === "loading" ? [provider] : [],
    generatedAt: "2026-10-26T00:00:00.000Z",
    entries: [
      {
        provider,
        status,
        enabled: true,
        models: status === "ready" ? [{ provider, id: "test", label: "Test" }] : [],
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
    host.client.getProvidersSnapshot.mockReset();
    host.client.refreshProvidersSnapshot.mockReset().mockResolvedValue({ requestId: "refresh" });
    queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  });

  afterEach(() => {
    cleanup();
    queryClient.clear();
    notifyManager.setScheduler(defaultScheduler);
    vi.useRealTimers();
  });

  it.each(["ready", "error", "unavailable"] as const)(
    "rechecks loading until %s and then slows down",
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

  it("retains settled catalogs without a periodic discovery timer", async () => {
    const original = snapshot("ready");
    queryClient.setQueryData(providersSnapshotQueryKey(serverId, cwd), original);
    const { result } = renderHook(() => useProvidersSnapshot(serverId, { cwd }), { wrapper });
    await act(() => vi.advanceTimersByTimeAsync(3_600_000));
    expect(host.client.getProvidersSnapshot).not.toHaveBeenCalled();
    expect(host.client.refreshProvidersSnapshot).not.toHaveBeenCalled();
    expect(result.current.entries).toEqual(original.entries);
  });

  it("refreshes OMP on access, shares concurrent work, and keeps cached choices usable", async () => {
    const original = snapshot("ready", 1, "omp");
    queryClient.setQueryData(providersSnapshotQueryKey(serverId, cwd), original);
    let finish!: (value: ReturnType<typeof snapshot>) => void;
    host.client.getProvidersSnapshot.mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const first = renderHook(() => useProvidersSnapshot(serverId, { cwd }), { wrapper });
    await act(() => vi.advanceTimersByTimeAsync(0));
    const second = renderHook(() => useProvidersSnapshot(serverId, { cwd }), { wrapper });
    act(() => {
      first.result.current.refetchIfStale("omp");
      second.result.current.refetchIfStale("omp");
    });
    await act(() => vi.advanceTimersByTimeAsync(0));
    expect(host.client.refreshProvidersSnapshot).toHaveBeenCalledExactlyOnceWith({
      cwd,
      providers: ["omp"],
    });
    expect(host.client.getProvidersSnapshot).toHaveBeenCalledTimes(1);
    expect(first.result.current.entries).toEqual(original.entries);
    expect(second.result.current.isLoading).toBe(false);
    const updated = snapshot("ready", 2, "omp");
    updated.entries[0].models[0].id = "new-model";
    await act(async () => {
      finish(updated);
    });
    expect(first.result.current.entries).toEqual(updated.entries);
    expect(second.result.current.entries).toEqual(updated.entries);
    await act(() => vi.advanceTimersByTimeAsync(3_600_000));
    expect(host.client.refreshProvidersSnapshot).toHaveBeenCalledTimes(1);
    host.client.getProvidersSnapshot.mockResolvedValue(updated);
    act(() => first.result.current.refetchIfStale("omp"));
    await act(() => vi.advanceTimersByTimeAsync(0));
    expect(host.client.refreshProvidersSnapshot).toHaveBeenCalledTimes(2);
  });

  it("keeps access refreshes separate for different working directories", async () => {
    for (const scope of [cwd, "/other-repo"]) {
      queryClient.setQueryData(providersSnapshotQueryKey(serverId, scope), {
        ...snapshot("ready", 1, "omp"),
        cwd: scope,
      });
    }
    host.client.getProvidersSnapshot.mockImplementation(async (options: { cwd: string }) => ({
      ...snapshot("ready", 2, "omp"),
      cwd: options.cwd,
    }));
    renderHook(() => useProvidersSnapshot(serverId, { cwd }), { wrapper });
    renderHook(() => useProvidersSnapshot(serverId, { cwd: "/other-repo" }), { wrapper });
    await act(() => vi.advanceTimersByTimeAsync(0));
    expect(host.client.refreshProvidersSnapshot).toHaveBeenCalledTimes(2);
    expect(host.client.refreshProvidersSnapshot).toHaveBeenCalledWith({ cwd, providers: ["omp"] });
    expect(host.client.refreshProvidersSnapshot).toHaveBeenCalledWith({
      cwd: "/other-repo",
      providers: ["omp"],
    });
  });

  it("joins cold OMP discovery without queuing another native bootstrap", async () => {
    host.client.getProvidersSnapshot
      .mockResolvedValueOnce(snapshot("loading", 1, "omp"))
      .mockResolvedValue(snapshot("ready", 2, "omp"));
    const { result } = renderHook(() => useProvidersSnapshot(serverId, { cwd }), { wrapper });
    await act(() => vi.advanceTimersByTimeAsync(0));
    expect(result.current.entries?.[0]?.status).toBe("ready");
    expect(host.client.refreshProvidersSnapshot).not.toHaveBeenCalled();
    expect(host.client.getProvidersSnapshot).toHaveBeenCalledTimes(2);
  });

  it("keeps old models after background failure and retries on the next access", async () => {
    const original = snapshot("ready", 1, "omp");
    queryClient.setQueryData(providersSnapshotQueryKey(serverId, cwd), original);
    host.client.refreshProvidersSnapshot.mockRejectedValueOnce(new Error("disconnected"));
    const { result } = renderHook(() => useProvidersSnapshot(serverId, { cwd }), { wrapper });
    await act(() => vi.advanceTimersByTimeAsync(0));
    expect(result.current.entries).toEqual(original.entries);
    expect(result.current.isLoading).toBe(false);
    expect(host.client.getProvidersSnapshot).not.toHaveBeenCalled();
    host.client.getProvidersSnapshot.mockResolvedValue(snapshot("ready", 2, "omp"));
    act(() => result.current.refetchIfStale("omp"));
    await act(() => vi.advanceTimersByTimeAsync(0));
    expect(host.client.refreshProvidersSnapshot).toHaveBeenCalledTimes(2);
    expect(queryClient.getQueryData(providersSnapshotQueryKey(serverId, cwd))).toMatchObject({
      revision: 2,
    });
  });

  it.each(["enabled", "connected", "active"] as const)(
    "starts OMP access refresh only when %s permits it",
    async (flag) => {
      queryClient.setQueryData(
        providersSnapshotQueryKey(serverId, cwd),
        snapshot("ready", 1, "omp"),
      );
      host.client.getProvidersSnapshot.mockResolvedValue(snapshot("ready", 2, "omp"));
      if (flag !== "enabled") host[flag] = false;
      const { result, rerender } = renderHook(
        ({ enabled }) => useProvidersSnapshot(serverId, { cwd, enabled }),
        {
          wrapper,
          initialProps: { enabled: flag !== "enabled" },
        },
      );
      act(() => result.current.refetchIfStale("omp"));
      await act(() => vi.advanceTimersByTimeAsync(3_600_000));
      expect(host.client.refreshProvidersSnapshot).not.toHaveBeenCalled();
      if (flag !== "enabled") host[flag] = true;
      rerender({ enabled: true });
      await act(() => vi.advanceTimersByTimeAsync(0));
      expect(host.client.refreshProvidersSnapshot).toHaveBeenCalledTimes(1);
    },
  );

  it("allows an explicit access even when automatic access refresh is disabled", async () => {
    queryClient.setQueryData(providersSnapshotQueryKey(serverId, cwd), snapshot("ready", 1, "omp"));
    host.client.getProvidersSnapshot.mockResolvedValue(snapshot("ready", 2, "omp"));
    const { result } = renderHook(
      () => useProvidersSnapshot(serverId, { cwd, refreshOnAccess: false }),
      { wrapper },
    );
    await act(() => vi.advanceTimersByTimeAsync(0));
    expect(host.client.refreshProvidersSnapshot).not.toHaveBeenCalled();
    act(() => result.current.refetchIfStale("omp"));
    await act(() => vi.advanceTimersByTimeAsync(0));
    expect(host.client.refreshProvidersSnapshot).toHaveBeenCalledTimes(1);
  });

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
});
