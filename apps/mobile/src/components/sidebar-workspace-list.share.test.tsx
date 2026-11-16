// @vitest-environment jsdom
import React from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ToastApiProvider } from "@/contexts/toast-context";
import type { SidebarWorkspaceEntry } from "@/hooks/use-sidebar-workspaces-list";
import type { SidebarGroupMode } from "@/stores/sidebar-view-store";
import { SidebarWorkspaceList } from "./sidebar-workspace-list";

// The menus are replaced by plain buttons for whichever Share handler the row hands them, so the
// test exercises the row's wiring: the handler it passes and the sheet it renders.
vi.mock("@/components/sidebar/sidebar-workspace-menu", () => ({
  SidebarWorkspaceContextMenu: ({
    children,
    onShare,
    workspaceKey,
    testID,
  }: React.PropsWithChildren<{ onShare?: () => void; workspaceKey: string; testID?: string }>) => (
    <div data-testid={testID}>
      {onShare ? (
        <button data-testid={`context-share-${workspaceKey}`} onClick={onShare}>
          Share
        </button>
      ) : null}
      {children}
    </div>
  ),
  SidebarWorkspaceMenu: ({
    onShare,
    workspaceKey,
  }: {
    onShare?: () => void;
    workspaceKey: string;
  }) =>
    onShare ? (
      <button data-testid={`kebab-share-${workspaceKey}`} onClick={onShare}>
        Share
      </button>
    ) : null,
}));
vi.mock("@/components/sharing/share-manager-sheet", () => ({
  ShareManagerSheet: ({
    visible,
    target,
  }: {
    visible: boolean;
    target: { resourceKind: string; resourceKey: string; displayName: string } | null;
  }) =>
    visible && target ? (
      <div data-testid="share-sheet">
        {`${target.resourceKind}:${target.resourceKey}:${target.displayName}`}
      </div>
    ) : null,
}));
vi.mock("@/hooks/use-keyboard-action-handler", () => ({ useKeyboardActionHandler: () => {} }));
vi.mock("@/components/workspace-hover-card", () => ({
  WorkspaceHoverCard: ({ children }: React.PropsWithChildren) => children,
}));

const workspace: SidebarWorkspaceEntry = {
  workspaceKey: "host:workspace-a",
  serverId: "host",
  workspaceId: "workspace-a",
  projectViewKey: "project",
  projectName: "project",
  projectRootPath: "/project",
  projectKind: "git",
  workspaceKind: "worktree",
  name: "Feature A",
  workspaceDirectory: "/project/workspace-a",
  workspaceDirectoryLabel: "workspace-a",
  title: null,
  pinnedAt: "2026-11-14T00:00:00.000Z",
  currentBranch: null,
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

const toastApi = { show: vi.fn(), copied: vi.fn(), error: vi.fn() };

function renderPinnedRow(groupMode: SidebarGroupMode) {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <ToastApiProvider api={toastApi}>
        <SidebarWorkspaceList
          workspaceGroups={[]}
          projectIconTargets={[]}
          pinnedGroups={{ pinnedChats: [workspace], unpinnedProjects: [] }}
          projects={[]}
          hasProjectsBeforeFilter={false}
          hasActiveProjectFilter={false}
          workspaceEntriesByKey={new Map([[workspace.workspaceKey, workspace]])}
          collapsedProjectKeys={new Set()}
          onToggleProjectCollapsed={() => {}}
          shortcutIndexByWorkspaceKey={new Map()}
          groupMode={groupMode}
        />
      </ToastApiProvider>
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  vi.stubGlobal("React", React);
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe.each<SidebarGroupMode>(["project", "status"])("%s-mode workspace row", (groupMode) => {
  it("opens the share sheet for its workspace when Share is chosen", () => {
    renderPinnedRow(groupMode);
    expect(screen.getByTestId(`sidebar-workspace-row-${workspace.workspaceKey}`)).toBeTruthy();
    expect(screen.queryByTestId("share-sheet")).toBeNull();

    act(() => {
      fireEvent.click(screen.getByTestId(`context-share-${workspace.workspaceKey}`));
    });
    expect(screen.getByTestId("share-sheet").textContent).toBe("workspace:workspace-a:Feature A");
  });
});
