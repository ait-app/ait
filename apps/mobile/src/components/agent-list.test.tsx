// @vitest-environment jsdom
import React from "react";
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { LayoutChangeEvent } from "react-native";
import type { AggregatedAgent } from "@/hooks/use-aggregated-agents";
import { AgentList } from "./agent-list";

const layout = vi.hoisted(() => ({
  isCompact: false,
  onLayout: null as ((event: LayoutChangeEvent) => void) | null,
}));

vi.mock("@/constants/layout", () => ({ useIsCompactFormFactor: () => layout.isCompact }));
vi.mock("@/components/provider-icons", () => ({ getProviderIcon: () => () => null }));
vi.mock("@/hooks/use-archive-agent", () => ({
  useArchiveAgent: () => ({ archiveAgent: vi.fn() }),
}));
vi.mock("@/stores/session-store", () => ({ useSessionStore: () => null }));
vi.mock("@/navigation/navigate-to-agent", () => ({ navigateToAgent: vi.fn() }));
// jsdom has no layout, so the list renders its rows directly and the test reports its width.
vi.mock("react-native", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-native")>();
  return {
    ...actual,
    FlatList: ({
      data,
      renderItem,
      onLayout,
    }: {
      data: { key: string }[];
      renderItem: (info: { item: { key: string } }) => React.ReactNode;
      onLayout: (event: LayoutChangeEvent) => void;
    }) => {
      layout.onLayout = onLayout;
      return (
        <div>
          {data.map((item) => (
            <React.Fragment key={item.key}>{renderItem({ item })}</React.Fragment>
          ))}
        </div>
      );
    },
  };
});

const agent = {
  id: "agent-1",
  serverId: "host-1",
  serverLabel: "Build host",
  title: "Fix the login flow",
  status: "idle",
  provider: "codex",
  lastActivityAt: new Date(),
  projectPlacement: {
    projectName: "ait",
    workspaceName: "main",
    checkout: { currentBranch: "fix/login" },
  },
} as unknown as AggregatedAgent;

function reportListWidth(width: number) {
  act(() => {
    layout.onLayout?.({
      nativeEvent: { layout: { x: 0, y: 0, width, height: 600 } },
    } as LayoutChangeEvent);
  });
}

// Only the stacked row joins its meta fields with separators.
function isStacked() {
  return screen.queryAllByText("·").length > 0;
}

beforeEach(() => {
  layout.isCompact = false;
  layout.onLayout = null;
});
afterEach(cleanup);

describe("AgentList row layout", () => {
  it("uses the form factor until the list has been measured", () => {
    layout.isCompact = true;
    render(<AgentList agents={[agent]} showHostColumn />);
    expect(isStacked()).toBe(true);
    cleanup();
    layout.isCompact = false;
    render(<AgentList agents={[agent]} showHostColumn />);
    expect(isStacked()).toBe(false);
  });

  it("stacks rows in a list too narrow for the host, project, branch and time columns", () => {
    render(<AgentList agents={[agent]} showHostColumn />);
    reportListWidth(660);
    expect(isStacked()).toBe(true);
    expect(screen.getByTestId("agent-row-title-host-1-agent-1").textContent).toBe(
      "Fix the login flow",
    );
    expect(screen.getByText("Build host")).toBeTruthy();
    reportListWidth(1000);
    expect(isStacked()).toBe(false);
    expect(screen.getByText("Build host")).toBeTruthy();
  });

  it("keeps the wide row at a width that only fits it without the host column", () => {
    render(<AgentList agents={[agent]} />);
    reportListWidth(760);
    expect(isStacked()).toBe(false);
    cleanup();
    render(<AgentList agents={[agent]} showHostColumn />);
    reportListWidth(760);
    expect(isStacked()).toBe(true);
  });
});
