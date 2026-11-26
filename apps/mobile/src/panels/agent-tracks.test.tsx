/** @vitest-environment jsdom */
import React from "react";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import { useSessionStore } from "@/stores/session-store";
import { normalizeAgentSnapshot } from "@/utils/agent-snapshots";
import { AgentTracks } from "./agent-tracks";

const { sendMessage } = vi.hoisted(() => ({ sendMessage: vi.fn().mockResolvedValue(undefined) }));
vi.mock("@/runtime/host-runtime", async (original) => ({
  ...(await original<typeof import("@/runtime/host-runtime")>()),
  useHostRuntimeClient: () => ({ sendMessage }),
  useHostRuntimeIsConnected: () => true,
}));
vi.mock("@/hooks/use-settings", () => ({ useSettings: () => ({}) }));
vi.mock("@/panels/pane-context", () => ({ usePaneContext: () => ({ tabId: "tab" }) }));
vi.mock("@/composer/workspace-diff-stat", () => ({ useWorkspaceHasDiffStat: () => false }));
vi.mock("@/composer/diff-stat-pill", () => ({ WorkspaceDiffStatPill: () => null }));
vi.mock("@/composer/goal", () => ({ AgentGoalTrack: () => null, useAgentGoal: () => null }));
vi.mock("@/subagents", () => ({
  useArchiveSubagent: () => vi.fn(),
  useDetachSubagent: () => vi.fn(),
  useSubagentOpensInSheet: () => () => false,
}));
vi.mock("@/subagents/track", () => ({ SubagentsTrack: () => null }));

afterEach(cleanup);

it.each(["directory", "details"])(
  "keeps an enabled Vibe accessible without tasks, children or a diff (%s)",
  async (source) => {
    vi.stubGlobal("React", React);
    await i18n.changeLanguage("zh-CN");
    const serverId = `vibe-only-${source}`;
    const agent = normalizeAgentSnapshot(
      {
        id: "parent",
        provider: "omp",
        cwd: "/repo",
        model: null,
        status: "idle",
        createdAt: "2026-11-26T00:00:00Z",
        updatedAt: "2026-11-26T00:00:00Z",
        lastUserMessageAt: null,
        currentModeId: null,
        availableModes: [],
        pendingPermissions: [],
        persistence: null,
        title: "Parent",
        labels: {},
        capabilities: {
          supportsStreaming: true,
          supportsSessionPersistence: true,
          supportsDynamicModes: true,
          supportsMcpServers: false,
          supportsReasoningStream: true,
          supportsToolInvocations: true,
        },
        runtimeInfo: {
          provider: "omp",
          sessionId: "native-parent",
          extra: { vibe: { enabled: false, workers: [] } },
        },
      },
      serverId,
    );
    const store = useSessionStore.getState();
    store.initializeSession(serverId, null);
    const publish = (enabled: boolean) => {
      const next = {
        ...agent,
        runtimeInfo: { ...agent.runtimeInfo!, extra: { vibe: { enabled, workers: [] } } },
      };
      if (source === "directory") store.setAgents(serverId, new Map([[agent.id, next]]));
      else store.setAgentDetails(serverId, new Map([[agent.id, next]]));
    };
    publish(false);
    render(
      <AgentTracks
        serverId={serverId}
        agentId="parent"
        workspaceId="workspace"
        cwd="/repo"
        subagentRows={[]}
        tasks={undefined}
        archiveFinishedStatus={{ kind: "idle" }}
        onArchiveFinished={vi.fn()}
      />,
    );
    expect(screen.queryByTestId("agent-vibe-pill")).toBeNull();
    act(() => publish(true));
    fireEvent.click(await screen.findByTestId("agent-vibe-pill"));
    expect(await screen.findByText("尚未分配任务")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "退出 Vibe" }));
    await waitFor(() => expect(sendMessage).toHaveBeenCalledWith("parent", "/vibe off"));
    expect(screen.getByTestId("agent-vibe-pill")).toBeTruthy();
    act(() => publish(false));
    expect(screen.queryByTestId("agent-vibe-pill")).toBeNull();
  },
);
