/** @vitest-environment jsdom */
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { OpenWorkspaceInNewWindowMenuItem } from "./menu-item";

const mocks = vi.hoisted(() => ({
  isElectron: true,
  bridgeAvailable: true,
  openNew: vi.fn(),
  toastError: vi.fn(),
}));

vi.mock("@/constants/platform", () => ({ getIsElectron: () => mocks.isElectron }));
vi.mock("@/desktop/host", () => ({
  getDesktopHost: () => (mocks.bridgeAvailable ? { window: { openNew: mocks.openNew } } : null),
}));
vi.mock("@/contexts/toast-context", () => ({
  useToast: () => ({ error: mocks.toastError }),
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

interface MenuItemProps {
  children: ReactNode;
  onSelect: () => void;
  testID: string;
}

vi.mock("@/components/ui/context-menu", () => ({
  ContextMenuItem: ({ children, onSelect, testID }: MenuItemProps) => (
    <button data-testid={testID} data-surface="context" onClick={onSelect}>
      {children}
    </button>
  ),
}));
vi.mock("@/components/ui/dropdown-menu", () => ({
  DropdownMenuItem: ({ children, onSelect, testID }: MenuItemProps) => (
    <button data-testid={testID} data-surface="dropdown" onClick={onSelect}>
      {children}
    </button>
  ),
}));

beforeEach(() => {
  mocks.isElectron = true;
  mocks.bridgeAvailable = true;
  mocks.openNew.mockReset().mockResolvedValue(undefined);
  mocks.toastError.mockReset();
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("open workspace in another desktop window", () => {
  it.each(["context", "dropdown"] as const)(
    "opens the exact remote workspace from the %s menu",
    async (surface) => {
      render(
        <OpenWorkspaceInNewWindowMenuItem
          serverId="remote-host"
          workspaceId="opaque/workspace-id"
          surface={surface}
          testID="open-window"
        />,
      );
      const item = screen.getByTestId("open-window");
      expect(item.getAttribute("data-surface")).toBe(surface);
      fireEvent.click(item);
      await waitFor(() =>
        expect(mocks.openNew).toHaveBeenCalledExactlyOnceWith({
          workspace: { serverId: "remote-host", workspaceId: "opaque/workspace-id" },
        }),
      );
    },
  );

  it.each([
    { isElectron: false, bridgeAvailable: true, serverId: "host", workspaceId: "workspace" },
    { isElectron: true, bridgeAvailable: false, serverId: "host", workspaceId: "workspace" },
    { isElectron: true, bridgeAvailable: true, serverId: " ", workspaceId: "workspace" },
    { isElectron: true, bridgeAvailable: true, serverId: "host", workspaceId: undefined },
  ])("hides the action when it cannot open a target: %j", (input) => {
    mocks.isElectron = input.isElectron;
    mocks.bridgeAvailable = input.bridgeAvailable;
    render(
      <OpenWorkspaceInNewWindowMenuItem
        serverId={input.serverId}
        workspaceId={input.workspaceId}
        surface="context"
        testID="open-window"
      />,
    );
    expect(screen.queryByTestId("open-window")).toBeNull();
  });

  it("reports an IPC failure through the existing new-window error toast", async () => {
    mocks.openNew.mockRejectedValue(new Error("window creation failed"));
    vi.spyOn(console, "warn").mockImplementation(() => {});
    render(
      <OpenWorkspaceInNewWindowMenuItem
        serverId="host"
        workspaceId="workspace"
        surface="context"
        testID="open-window"
      />,
    );
    fireEvent.click(screen.getByTestId("open-window"));
    await waitFor(() =>
      expect(mocks.toastError).toHaveBeenCalledExactlyOnceWith(
        "sidebar.project.actions.openNewWindowFailed",
      ),
    );
  });
});
