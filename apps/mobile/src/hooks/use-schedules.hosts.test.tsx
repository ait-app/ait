/** @vitest-environment jsdom */
import { cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useSchedules } from "./use-schedules";

const state = vi.hoisted(() => ({
  query: vi.fn(),
  statuses: vi.fn(
    () =>
      new Map([
        ["a", "connecting"],
        ["b", "online"],
      ]),
  ),
  list: vi.fn(async () => ({ schedules: [{ id: "schedule-b" }], error: null })),
}));

vi.mock("@/data/query", () => ({
  useFetchQuery: (options: unknown) => {
    state.query(options);
    return {};
  },
}));
vi.mock("@/runtime/host-runtime", () => ({
  useHosts: () =>
    ["a", "b"].map((serverId) => ({
      serverId,
      label: "Shared workstation",
      connections: [
        {
          type: "accountRelay",
          center: "https://center.test",
          hostId: "host",
          shareId: serverId,
          shareResourceKind: "node",
        },
      ],
    })),
  useHostRuntimeConnectionStatuses: state.statuses,
  getHostRuntimeStore: () => ({
    getClient: (serverId: string) => (serverId === "b" ? { scheduleList: state.list } : null),
    getSnapshot: (serverId: string) => ({
      connectionStatus: serverId === "b" ? "online" : "connecting",
    }),
  }),
}));

afterEach(cleanup);

it("loads schedules through an online non-representative grant in a shared host group", async () => {
  renderHook(useSchedules);
  expect(state.statuses).toHaveBeenCalledWith(["a", "b"]);
  const { queryFn } = state.query.mock.calls[0][0];
  await expect(queryFn()).resolves.toMatchObject({
    status: "loaded",
    data: [{ id: "schedule-b", serverId: "b", serverName: "Shared workstation" }],
  });
  expect(state.list).toHaveBeenCalledOnce();
});
