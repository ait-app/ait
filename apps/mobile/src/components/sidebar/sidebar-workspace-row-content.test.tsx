// @vitest-environment jsdom
import React from "react";
import { cleanup, fireEvent, render } from "@testing-library/react";
import { Text, View } from "react-native";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SidebarWorkspaceEntry } from "@/hooks/use-sidebar-workspaces-list";
import { SidebarWorkspaceRowFrame } from "./sidebar-workspace-row-content";

const layout = vi.hoisted(() => ({ compact: false }));
vi.mock("@/constants/layout", () => ({ useIsCompactFormFactor: () => layout.compact }));
vi.mock("@/components/workspace-hover-card", () => ({
  WorkspaceHoverCard: ({ children }: React.PropsWithChildren) => children,
}));
vi.mock("@/components/sidebar/project-leading-visual", () => ({
  ProjectStatusIndicator: () => null,
}));
vi.mock("@/components/sidebar/workspace-meta-row", () => ({ WorkspaceMetaRow: () => null }));
vi.mock("@/components/sidebar/workspace-trailing", () => ({
  hasSidebarWorkspaceTrailing: () => false,
}));
vi.mock("@/components/status-ring", () => ({ StatusRing: () => null }));
vi.mock("@/hooks/use-settings", () => ({ useAppSettings: () => ({ settings: {} }) }));
vi.mock("@/workspace-labels", () => ({ useWorkspaceLabelDefinitions: () => [] }));

afterEach(cleanup);
beforeEach(() => {
  layout.compact = false;
});

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
  workspaceDirectoryLabel: "workspace",
  title: null,
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

function Row({ id, isDragging = false }: { id: string; isDragging?: boolean }) {
  return (
    <SidebarWorkspaceRowFrame workspace={workspace} isDragging={isDragging}>
      {({ isHovered, hoverHandlers, onContextMenuOpenChange }) => (
        <View {...hoverHandlers} testID={id}>
          <Text testID={`${id}-state`}>{isHovered ? "highlighted" : "idle"}</Text>
          <button onClick={() => onContextMenuOpenChange(true)}>open menu</button>
          <button onClick={() => onContextMenuOpenChange(false)}>close menu</button>
        </View>
      )}
    </SidebarWorkspaceRowFrame>
  );
}

function pointerEnter(element: HTMLElement, pointerType: string) {
  const event = new Event("pointerover", { bubbles: true });
  Object.defineProperty(event, "pointerType", { value: pointerType });
  fireEvent(element, event);
}

describe("sidebar workspace hover", () => {
  it("does not leave multiple rows highlighted when fingers enter them", () => {
    const view = render(
      <>
        <Row id="first" />
        <Row id="second" />
      </>,
    );

    pointerEnter(view.getByTestId("first"), "touch");
    pointerEnter(view.getByTestId("second"), "touch");

    expect(view.getByTestId("first-state").textContent).toBe("idle");
    expect(view.getByTestId("second-state").textContent).toBe("idle");
  });

  it("keeps mouse hover feedback and clears it on leave", () => {
    const view = render(<Row id="row" />);
    const row = view.getByTestId("row");

    pointerEnter(row, "mouse");
    expect(view.getByTestId("row-state").textContent).toBe("highlighted");
    fireEvent.pointerOut(row);
    expect(view.getByTestId("row-state").textContent).toBe("idle");
  });

  it("does not show hover in the mobile sidebar, including after a layout change", () => {
    const view = render(<Row id="row" />);
    pointerEnter(view.getByTestId("row"), "mouse");
    expect(view.getByTestId("row-state").textContent).toBe("highlighted");

    layout.compact = true;
    view.rerender(<Row id="row" />);
    pointerEnter(view.getByTestId("row"), "mouse");
    expect(view.getByTestId("row-state").textContent).toBe("idle");

    layout.compact = false;
    view.rerender(<Row id="row" />);
    expect(view.getByTestId("row-state").textContent).toBe("idle");
  });

  it("clears hover when a context menu opens and keeps it cleared on close", () => {
    const view = render(<Row id="row" />);
    pointerEnter(view.getByTestId("row"), "mouse");
    expect(view.getByTestId("row-state").textContent).toBe("highlighted");

    fireEvent.click(view.getByText("open menu"));
    expect(view.getByTestId("row-state").textContent).toBe("idle");
    fireEvent.click(view.getByText("close menu"));
    expect(view.getByTestId("row-state").textContent).toBe("idle");
  });

  it("clears hover when dragging starts and keeps it cleared after release", () => {
    const view = render(<Row id="row" />);
    pointerEnter(view.getByTestId("row"), "mouse");
    expect(view.getByTestId("row-state").textContent).toBe("highlighted");

    view.rerender(<Row id="row" isDragging />);
    expect(view.getByTestId("row-state").textContent).toBe("idle");
    view.rerender(<Row id="row" />);
    expect(view.getByTestId("row-state").textContent).toBe("idle");
  });
});
