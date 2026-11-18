import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import { TimelineExport } from "./timeline-export";

const fixture = vi.hoisted(() => ({
  supported: true,
  start: vi.fn(),
  client: { exportAgentTimeline: vi.fn(), cancelAgentTimelineExport: vi.fn() },
}));
vi.mock("@/runtime/host-runtime", () => {
  const snapshot = () => ({
    connectionStatus: "online",
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

it("hides the download control and shows unavailability on an old host", async () => {
  fixture.supported = false;
  await act(async () => root.render(<TimelineExport serverId="host" agentId="agent" />));
  expect(container.textContent).toContain("Export unavailable");
  expect(container.querySelector('[data-testid="timeline-export-download"]')).toBeNull();
  expect(fixture.start).not.toHaveBeenCalled();
});

it("starts an export with the selected format and scope", async () => {
  await act(async () =>
    container.querySelector<HTMLElement>('[data-testid="timeline-export-download"]')!.click(),
  );
  expect(fixture.start).toHaveBeenCalledExactlyOnceWith(
    expect.objectContaining({
      serverId: "host",
      agentId: "agent",
      activeConnectionId: "direct",
      generate: expect.any(Function),
      release: expect.any(Function),
    }),
  );
  const { generate, release } = fixture.start.mock.calls[0]![0];
  generate();
  expect(fixture.client.exportAgentTimeline).toHaveBeenCalledExactlyOnceWith("agent", {
    format: "markdown",
    scope: "conversation",
  });
  release("token");
  expect(fixture.client.cancelAgentTimelineExport).toHaveBeenCalledExactlyOnceWith(
    "agent",
    "token",
  );
});
