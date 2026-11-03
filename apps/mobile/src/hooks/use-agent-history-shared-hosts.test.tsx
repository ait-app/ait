// @vitest-environment jsdom
import { cleanup, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { afterEach, expect, it, vi } from "vitest";
import type { HostChoice } from "@/hosts/host-groups";
import { createAccountRelayHostConnection } from "@/types/host-connection";
import { useAgentHistory } from "./use-agent-history";

const state = vi.hoisted(() => ({
  hosts: [] as HostChoice[],
  fetch: vi.fn(async (_serverId: string) => ({
    requestId: "history",
    entries: [],
    pageInfo: { nextCursor: null, prevCursor: null, hasMore: false },
  })),
}));
const runtime = {
  subscribeAll: () => () => {},
  getVersion: () => 0,
  getSnapshot: () => ({ connectionStatus: "online" }),
  getClient: (serverId: string) => ({ fetchAgentHistory: () => state.fetch(serverId) }),
};
vi.mock("@/runtime/host-runtime", () => ({
  useHosts: () => state.hosts,
  getHostRuntimeStore: () => runtime,
  isHostRuntimeConnected: () => true,
}));

afterEach(cleanup);

it("fetches every grant when History filters to one physical Host", async () => {
  const hostId = "11111111-1111-4111-8111-111111111111";
  const grant = (shareId: string, scope: "workspace" | "project"): HostChoice => ({
    serverId: `shared:${shareId}`,
    label: "Shared Host",
    connections: [
      createAccountRelayHostConnection({
        center: "https://center.test/api",
        hostId,
        shareId,
        nodeId: "node",
        shareResourceKind: scope,
      }),
    ],
  });
  const workspace = grant("22222222-2222-4222-8222-222222222222", "workspace");
  const project = grant("33333333-3333-4333-8333-333333333333", "project");
  state.hosts = [workspace, project, { serverId: "unrelated", label: "Another Host" }];
  state.fetch.mockClear();
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  const view = renderHook(({ serverId }) => useAgentHistory({ serverId }), {
    initialProps: { serverId: project.serverId },
    wrapper,
  });
  await waitFor(() => expect(view.result.current.isInitialLoad).toBe(false));
  expect(state.fetch.mock.calls.map(([serverId]) => serverId).sort()).toEqual(
    [workspace.serverId, project.serverId].sort(),
  );
  expect(view.result.current.hostErrors).toEqual([]);

  // Selecting the other alias is the same physical filter, so it shares the cache.
  view.rerender({ serverId: workspace.serverId });
  expect(state.fetch).toHaveBeenCalledTimes(2);

  state.hosts = [project];
  view.rerender({ serverId: project.serverId });
  await waitFor(() => expect(state.fetch).toHaveBeenCalledTimes(3));
  expect(state.fetch).toHaveBeenLastCalledWith(project.serverId);
  view.unmount();
  client.clear();
});
