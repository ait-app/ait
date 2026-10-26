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
});
