import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import { TimelineExport } from "./timeline-export";

const fixture = vi.hoisted(() => ({
  supported: true,
  epoch: 1,
  start: vi.fn(),
  client: { exportAgentTimeline: vi.fn(), cancelAgentTimelineExport: vi.fn() },
}));
vi.mock("@/runtime/host-runtime", () => {
  const snapshot = () => ({
    connectionStatus: "online",
    clientGeneration: 1,
    connectionEpoch: fixture.epoch,
    activeConnectionId: "direct",
  });
  return {
    useHosts: () => [],
    useHostRuntimeSnapshot: snapshot,
    getHostRuntimeStore: () => ({ getSnapshot: snapshot, getClient: () => fixture.client }),
  };
});
vi.mock("@/stores/session-store", () => {
  interface FixtureSessionState {
    sessions: { host: { serverInfo: { features: { agentTimelineExport: boolean } } } };
  }
  const state = () => ({
    sessions: { host: { serverInfo: { features: { agentTimelineExport: fixture.supported } } } },
  });
  return {
    useSessionStore: Object.assign(
      (select: (value: FixtureSessionState) => unknown) => select(state()),
      { getState: state },
    ),
  };
});
vi.mock("@/stores/download-store", () => {
  const state = { downloads: new Map(), startTimelineExport: fixture.start };
  return {
    useDownloadStore: Object.assign((select: (value: typeof state) => unknown) => select(state), {
      getState: () => state,
    }),
  };
});
let root: Root;
let container: HTMLDivElement;
beforeEach(async () => {
  await i18n.changeLanguage("en");
  vi.stubGlobal("React", React);
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  fixture.supported = true;
  fixture.epoch = 1;
  fixture.client.cancelAgentTimelineExport.mockResolvedValue({ released: true });
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  await act(async () => root.render(<TimelineExport serverId="host" agentId="agent" />));
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.clearAllMocks();
  vi.unstubAllGlobals();
});

it("rechecks a capability downgrade at click time without issuing an unsupported RPC", async () => {
  fixture.supported = false;
  await act(async () =>
    container.querySelector<HTMLElement>('[data-testid="timeline-export-download"]')!.click(),
  );
  expect(fixture.start).not.toHaveBeenCalled();
  expect(fixture.client.exportAgentTimeline).not.toHaveBeenCalled();
  await act(async () => root.render(<TimelineExport serverId="host" agentId="agent" />));
  expect(container.textContent).toContain("Export unavailable");
});

it("releases a late result from a previous connection instead of downloading or regenerating it", async () => {
  const result = { token: "late-token" };
  const pending = Promise.withResolvers<typeof result>();
  fixture.client.exportAgentTimeline.mockReturnValueOnce(pending.promise);
  await act(async () =>
    container.querySelector<HTMLElement>('[data-testid="timeline-export-download"]')!.click(),
  );
  const generate = fixture.start.mock.calls[0]![0].generate as () => Promise<unknown>;
  const generation = generate();
  const failed = expect(generation).rejects.toThrow("Export unavailable");
  fixture.epoch += 1;
  pending.resolve(result);
  await failed;
  expect(fixture.client.exportAgentTimeline).toHaveBeenCalledExactlyOnceWith("agent", {
    format: "markdown",
    scope: "conversation",
  });
  expect(fixture.client.cancelAgentTimelineExport).toHaveBeenCalledExactlyOnceWith(
    "agent",
    "late-token",
  );
});
