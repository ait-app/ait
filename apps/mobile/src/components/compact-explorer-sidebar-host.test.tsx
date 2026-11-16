// @vitest-environment jsdom
import { useState } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CompactExplorerSidebarHost } from "@/components/compact-explorer-sidebar-host";

vi.mock("@/constants/platform", () => ({ isWeb: false }));
vi.mock("@/stores/navigation-active-workspace-store", () => ({
  useActiveWorkspaceSelection: () => ({ serverId: "host", workspaceId: "workspace" }),
}));
vi.mock("@/stores/session-store-hooks", () => ({
  useWorkspace: () => ({ workspaceDirectory: "/workspace" }),
}));
vi.mock("@/components/compact-explorer-sidebar", () => ({
  CompactExplorerSidebar: () => <aside>Explorer overlay</aside>,
  NativeExplorerSidebarDock: () => <aside>Explorer dock</aside>,
}));
vi.mock("@/mobile-panels/gestures", () => ({ useOpenFileExplorerGesture: () => ({}) }));
vi.mock("@/mobile-panels/provider", () => ({ useIsMobilePanelActive: () => false }));
vi.mock("@/runtime/host-runtime", () => ({
  useHostRuntimeClient: () => null,
  useHostRuntimeIsConnected: () => false,
}));
vi.mock("@/stores/panel-store", () => ({ usePanelStore: () => vi.fn() }));
vi.mock("@/stores/workspace-layout-store", () => ({ useWorkspaceLayoutStore: () => vi.fn() }));
vi.mock("@/screens/workspace/use-workspace-checkout-status", () => ({
  useWorkspaceCheckoutStatus: () => ({ checkoutQuery: { data: null } }),
}));
vi.mock("@/screens/workspace/workspace-file-open-command", () => ({
  openWorkspaceFileFromExplorer: vi.fn(),
}));
vi.mock("@/git/diff-document/workspace-cache", () => ({
  DiffDocumentWorkspaceCacheProvider: ({ children }: { children: React.ReactNode }) => children,
}));
vi.mock("@/components/appearance-style-boundary", () => ({
  AppearanceStyleBoundary: ({ children }: { children: React.ReactNode }) => children,
}));
vi.mock("react-native-gesture-handler", () => ({
  GestureDetector: ({ children }: { children: React.ReactNode }) => children,
}));
afterEach(cleanup);

function TasksScreen() {
  const [selected, setSelected] = useState(false);
  return <button onClick={() => setSelected(true)}>{selected ? "Task detail" : "Tasks"}</button>;
}

describe("native explorer host viewport changes", () => {
  it("keeps navigation and screen state when the explorer changes between dock and overlay", () => {
    const view = (presentation: "dock" | "overlay", enabled = true) => (
      <CompactExplorerSidebarHost enabled={enabled} presentation={presentation}>
        <TasksScreen />
      </CompactExplorerSidebarHost>
    );
    const { rerender } = render(view("dock"));
    fireEvent.click(screen.getByText("Tasks"));
    for (const presentation of ["overlay", "dock", "overlay"] as const) {
      rerender(view(presentation));
      expect(screen.getByText("Task detail")).toBeTruthy();
    }
    // Leaving a workspace suppresses its explorer without replacing the route host.
    for (const enabled of [false, true, false]) {
      rerender(view("dock", enabled));
      expect(screen.getByText("Task detail")).toBeTruthy();
      expect(Boolean(screen.queryByText("Explorer dock"))).toBe(enabled);
    }
  });
});
