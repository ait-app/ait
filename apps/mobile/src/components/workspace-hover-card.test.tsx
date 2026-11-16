// @vitest-environment jsdom
import React from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { Text, View } from "react-native";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SidebarWorkspaceEntry } from "@/hooks/use-sidebar-workspaces-list";
import { WorkspaceHoverCard } from "./workspace-hover-card";

vi.mock("@/constants/platform", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/constants/platform")>()),
  isWeb: true,
}));
vi.mock("@/constants/layout", () => ({ useIsCompactFormFactor: () => false }));
vi.mock("@gorhom/portal", () => ({
  Portal: ({ children }: React.PropsWithChildren) => children,
}));
vi.mock("@gorhom/bottom-sheet", () => ({ useBottomSheetModalInternal: () => null }));
vi.mock("@/hooks/use-hover-safe-zone", () => ({ useHoverSafeZone: () => undefined }));
vi.mock("@/components/ui/menu/menu-anchor", () => ({
  measureElement: () => new Promise(() => {}),
}));
vi.mock("@/runtime/host-runtime", () => ({ useHosts: () => [] }));
vi.mock("@/utils/copy-to-clipboard", () => ({ copyToClipboard: vi.fn(async () => {}) }));
vi.mock("@/components/ui/floating", async () => {
  const { forwardRef } = await import("react");
  const { View: RNView } = await import("react-native");
  return {
    FloatingSurface: forwardRef<View, React.ComponentProps<typeof RNView>>(
      function FloatingSurface(props, ref) {
        return <RNView ref={ref} testID={props.testID} children={props.children} />;
      },
    ),
  };
});
vi.mock("lucide-react-native", async (importOriginal) => ({
  ...(await importOriginal<typeof import("lucide-react-native")>()),
  Check: () => <Text testID="icon-copied">copied</Text>,
}));

const workspace: SidebarWorkspaceEntry = {
  workspaceKey: "host:workspace",
  serverId: "host",
  workspaceId: "workspace",
  projectViewKey: "project",
  projectName: "project",
  projectKind: "git",
  workspaceKind: "worktree",
  name: "workspace",
  workspaceDirectory: "/project/workspace",
  workspaceDirectoryLabel: "",
  title: null,
  currentBranch: "feature/copy",
  statusBucket: "done",
  statusEnteredAt: null,
  archivingAt: null,
  diffStat: null,
  prHint: null,
  archiveHasUncommittedChanges: null,
  archiveUnpushedCommitCount: null,
  scripts: [],
  hasRunningScripts: false,
};

function openCard() {
  render(
    <WorkspaceHoverCard workspace={workspace}>
      <Text testID="trigger">row</Text>
    </WorkspaceHoverCard>,
  );
  fireEvent(screen.getByTestId("trigger"), new Event("pointerover", { bubbles: true }));
  return screen.getByTestId("hover-card-workspace-branch");
}

function copyBranch(branchText: HTMLElement) {
  act(() => {
    fireEvent.click(branchText);
  });
}

beforeEach(() => {
  vi.stubGlobal("React", React);
  vi.useFakeTimers();
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("workspace hover card copy feedback", () => {
  it("keeps the copied mark for the full interval after a second copy", () => {
    const branch = openCard();
    copyBranch(branch);
    expect(screen.queryByTestId("icon-copied")).not.toBeNull();

    act(() => {
      vi.advanceTimersByTime(1500);
    });
    copyBranch(branch);
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(screen.queryByTestId("icon-copied")).not.toBeNull();

    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(screen.queryByTestId("icon-copied")).toBeNull();
  });

  it("drops the pending reset when the card closes", () => {
    const branch = openCard();
    const timersBeforeCopy = vi.getTimerCount();
    copyBranch(branch);
    expect(vi.getTimerCount()).toBe(timersBeforeCopy + 1);

    cleanup();
    expect(vi.getTimerCount()).toBe(0);
  });
});
