// @vitest-environment jsdom
import React from "react";
import { act, cleanup, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { HostChoice } from "@/hosts/host-groups";
import { createAccountRelayHostConnection } from "@/types/host-connection";
import { useHostStats } from "./queries";

const state = vi.hoisted(() => ({ hosts: [] as HostChoice[], getHostStats: vi.fn() }));
vi.mock("@/hooks/use-app-visible", () => ({ useAppVisible: () => true }));
vi.mock("@/runtime/host-runtime", () => ({
  useHosts: () => state.hosts,
  useHostRuntimeIsConnected: () => true,
  getHostRuntimeStore: () => ({ getClient: () => ({ getHostStats: state.getHostStats }) }),
}));
vi.mock("@/stores/session-store", () => ({
  useSessionStore: (selector: (value: unknown) => unknown) =>
    selector({
      sessions: { shared: { serverInfo: { features: { hostStats: true } } } },
    }),
}));
vi.mock("@/stores/navigation-active-workspace-store", () => ({}));

beforeEach(() => {
  vi.stubGlobal("React", React);
  vi.useFakeTimers();
  state.getHostStats.mockReset().mockResolvedValue({ current: { at: 1 } });
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

it.each(["workspace", "project", "node"] as const)(
  "only polls whole-node shares even if the server advertises hostStats (%s)",
  async (scope) => {
    state.hosts = [
      {
        serverId: "shared",
        label: "Shared",
        connections: [
          createAccountRelayHostConnection({
            center: "https://center.test/api",
            hostId: "11111111-1111-4111-8111-111111111111",
            shareId: "22222222-2222-4222-8222-222222222222",
            nodeId: "node",
            shareResourceKind: scope,
          }),
        ],
      },
    ];
    const client = new QueryClient();
    const { result, unmount } = renderHook(() => useHostStats("shared", true, true), {
      wrapper: ({ children }) => (
        <QueryClientProvider client={client}>{children}</QueryClientProvider>
      ),
    });
    await act(() => vi.advanceTimersByTimeAsync(6000));
    expect(result.current.restricted).toBe(scope !== "node");
    if (scope === "node") expect(state.getHostStats.mock.calls.length).toBeGreaterThan(1);
    else expect(state.getHostStats).not.toHaveBeenCalled();
    unmount();
    client.clear();
  },
);
