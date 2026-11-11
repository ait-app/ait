import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { AgentSessionStatistics } from "@ait/protocol/messages";
import { i18n } from "@/i18n/i18next";
import { SessionStatistics } from "./session-statistics";

const host = vi.hoisted(() => ({
  supported: true,
  connected: true,
  epoch: 1,
  getAgentStatistics: vi.fn(),
}));
vi.mock("@/runtime/host-runtime", () => ({
  useHostRuntimeSnapshot: () => ({
    connectionStatus: host.connected ? "online" : "offline",
    clientGeneration: 1,
    connectionEpoch: host.epoch,
  }),
  getHostRuntimeStore: () => ({
    getClient: () => ({ getAgentStatistics: host.getAgentStatistics }),
  }),
}));
vi.mock("@/stores/session-store", () => {
  const getState = () => ({
    sessions: { host: { serverInfo: { features: { agentStatisticsGet: host.supported } } } },
  });
  type FixtureState = {
    sessions: { host: { serverInfo: { features: { agentStatisticsGet: boolean } } } };
  };
  return {
    useSessionStore: Object.assign(
      (selector: (state: FixtureState) => unknown) => selector(getState()),
      { getState },
    ),
  };
});
const snapshot: AgentSessionStatistics = {
  agentId: "selected",
  provider: "mock",
  status: "idle",
  configuredModel: "requested",
  effectiveModel: "actual",
  updatedAt: "2026-11-11T00:00:00Z",
  usageScope: "provider_snapshot",
  usage: {
    inputTokens: 12,
    cachedInputTokens: null,
    outputTokens: 0,
    totalCostUsd: 0,
    contextWindowMaxTokens: null,
    contextWindowUsedTokens: 0,
  },
};
let root: Root;
let container: HTMLDivElement;
let client: QueryClient;
async function render() {
  await act(async () => {
    root.render(
      <QueryClientProvider client={client}>
        <SessionStatistics serverId="host" agentId="selected" />
      </QueryClientProvider>,
    );
  });
}
beforeEach(async () => {
  await i18n.changeLanguage("en");
  vi.stubGlobal("React", React);
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  host.supported = true;
  host.connected = true;
  host.epoch = 1;
  host.getAgentStatistics.mockResolvedValue(snapshot);
  client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  client.clear();
  container.remove();
  vi.clearAllMocks();
  vi.unstubAllGlobals();
});
it("shows distinct models, null versus zero, and replaces repeated snapshots without summing", async () => {
  await render();
  await vi.waitFor(() => expect(container.textContent).toContain("Input tokens: 12"));
  expect(host.getAgentStatistics).toHaveBeenCalledWith("selected");
  expect(container.textContent).toContain("Configured model: requested");
  expect(container.textContent).toContain("Actual model: actual");
  expect(container.textContent).toContain("Cached input tokens: Unavailable");
  expect(container.textContent).toContain("Output tokens: 0");
  expect(container.textContent).toContain("Reported cost (USD): 0");
  expect(container.textContent).toContain("not a cumulative session total");
  expect(container.textContent).toContain("Child session usage is excluded");
  host.getAgentStatistics.mockResolvedValue({
    ...snapshot,
    usage: { ...snapshot.usage, inputTokens: 7 },
  });
  await act(async () => {
    await client.invalidateQueries({ queryKey: ["agent-statistics", "host", "selected"] });
  });
  await vi.waitFor(() => expect(container.textContent).toContain("Input tokens: 7"));
  expect(container.textContent).not.toContain("Input tokens: 19");
});
it("hides cached data on host downgrade and sends no unsupported request", async () => {
  await render();
  await vi.waitFor(() => expect(container.textContent).toContain("Input tokens: 12"));
  host.supported = false;
  host.getAgentStatistics.mockClear();
  await render();
  expect(container.textContent).toContain("Session statistics unavailable");
  expect(container.textContent).not.toContain("Input tokens: 12");
  expect(host.getAgentStatistics).not.toHaveBeenCalled();
});
it("does not dispatch on an old host", async () => {
  host.supported = false;
  await render();
  expect(container.textContent).toContain("Session statistics unavailable");
  expect(host.getAgentStatistics).not.toHaveBeenCalled();
});

it("ignores a late response from a previous connection epoch", async () => {
  const { promise, resolve: resolveOld } = Promise.withResolvers<AgentSessionStatistics>();
  host.getAgentStatistics.mockReturnValueOnce(promise);
  await render();
  host.epoch += 1;
  host.getAgentStatistics.mockResolvedValue({
    ...snapshot,
    usage: { ...snapshot.usage, inputTokens: 7 },
  });
  await render();
  await vi.waitFor(() => expect(container.textContent).toContain("Input tokens: 7"));
  await act(async () => {
    resolveOld(snapshot);
  });
  expect(container.textContent).not.toContain("Input tokens: 12");
  expect(container.textContent).toContain("Input tokens: 7");
});
