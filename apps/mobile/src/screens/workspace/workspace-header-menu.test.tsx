// @vitest-environment jsdom
import React, { type ReactNode } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { WorkspaceHeaderMenuDesktop } from "./workspace-header-menu";

const { archiveState } = vi.hoisted(() => ({
  archiveState: { archive: vi.fn(), canArchive: true, isArchiving: false },
}));

vi.mock("@/workspace/use-workspace-screen-archive", () => ({
  useWorkspaceScreenArchive: () => archiveState,
}));
vi.mock("@/hooks/use-shortcut-keys", () => ({ useShortcutKeys: () => null }));
vi.mock("@/screens/workspace/scripts/workspace-scripts-menu-pages", () => ({
  useWorkspaceScriptsMenuPages: () => ({ pages: [], trigger: null, needsAttention: false }),
}));
vi.mock("@/components/ui/dropdown-menu", () => ({
  DropdownMenu: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  DropdownMenuContent: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  DropdownMenuTrigger: ({ children, testID }: { children: ReactNode; testID: string }) => (
    <button data-testid={testID}>{children}</button>
  ),
  DropdownMenuItem: ({
    children,
    testID,
    onSelect,
    disabled,
    status,
  }: {
    children: ReactNode;
    testID: string;
    onSelect: () => void;
    disabled?: boolean;
    status?: string;
  }) => (
    <button data-testid={testID} data-status={status} onClick={onSelect} disabled={disabled}>
      {children}
    </button>
  ),
  DropdownMenuSeparator: () => <hr />,
}));

function renderMenu() {
  render(
    <WorkspaceHeaderMenuDesktop
      serverId="server-1"
      workspaceId="workspace-1"
      currentBranchName="feature"
      showWorkspaceSetup={false}
      importAgentDisabled={false}
      copyPathDisabled={false}
      onOpenImportSheet={vi.fn()}
      onCopyWorkspacePath={vi.fn()}
      onCopyBranchName={vi.fn()}
      onOpenSetupTab={vi.fn()}
    />,
  );
}

beforeEach(() => {
  vi.stubGlobal("React", React);
  archiveState.archive.mockReset();
  archiveState.canArchive = true;
  archiveState.isArchiving = false;
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("workspace header menu", () => {
  it("archives the workspace from the last menu item", () => {
    renderMenu();

    const items = screen.getAllByRole("button").map((item) => item.getAttribute("data-testid"));
    expect(items.at(-1)).toBe("workspace-archive-action");
    fireEvent.click(screen.getByTestId("workspace-archive-action"));
    expect(archiveState.archive).toHaveBeenCalledOnce();
  });

  it("disables archive until the workspace can be archived", () => {
    archiveState.canArchive = false;
    renderMenu();

    expect(screen.getByTestId("workspace-archive-action")).toHaveProperty("disabled", true);
  });

  it("shows archive as pending and disabled while the workspace is archiving", () => {
    archiveState.isArchiving = true;
    renderMenu();

    const archive = screen.getByTestId("workspace-archive-action");
    expect(archive.getAttribute("data-status")).toBe("pending");
    expect(archive).toHaveProperty("disabled", true);
  });
});
