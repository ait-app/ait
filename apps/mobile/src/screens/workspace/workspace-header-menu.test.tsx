// @vitest-environment jsdom
import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { WorkspaceHeaderMenuMobile } from "./workspace-header-menu";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("@/hooks/use-daemon-config", () => ({ useDaemonConfig: () => ({ config: null }) }));
vi.mock("@/screens/workspace/scripts", () => ({
  useWorkspaceScriptsMenuPages: () => ({ pages: [], trigger: null, needsAttention: false }),
}));
vi.mock("@/components/ui/dropdown-menu", () => {
  const passthrough = ({ children }: { children?: ReactNode }) => children;
  return {
    DropdownMenu: passthrough,
    DropdownMenuContent: passthrough,
    DropdownMenuTrigger: () => null,
    DropdownMenuLabel: passthrough,
    DropdownMenuSeparator: () => null,
    DropdownMenuItem: ({
      children,
      onSelect,
      testID,
    }: {
      children: ReactNode;
      onSelect: () => void;
      testID: string;
    }) => (
      <button data-testid={testID} onClick={onSelect}>
        {children}
      </button>
    ),
  };
});

afterEach(() => vi.unstubAllGlobals());

it("creates a session in the current workspace from the mobile actions menu", () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const createSession = vi.fn();
  const createTerminal = vi.fn();
  const openImport = vi.fn();
  const container = document.createElement("div");
  const root = createRoot(container);
  try {
    act(() =>
      root.render(
        <WorkspaceHeaderMenuMobile
          normalizedServerId="host-1"
          currentBranchName="main"
          showWorkspaceSetup={false}
          showCreateBrowserTab={false}
          createTerminalDisabled={false}
          importAgentDisabled={false}
          copyPathDisabled={false}
          scripts={{
            serverId: "host-1",
            workspaceId: "workspace-1",
            scripts: [],
            liveTerminalIds: [],
            onScriptTerminalStarted: vi.fn(),
            onViewTerminal: vi.fn(),
            onOpenUrlInBrowserTab: vi.fn(),
          }}
          onCreateDraftTab={createSession}
          onCreateTerminalWithProfile={createTerminal}
          onCreateBrowser={vi.fn()}
          onCreateSchedule={vi.fn()}
          onOpenImportSheet={openImport}
          onCopyWorkspacePath={vi.fn()}
          onCopyBranchName={vi.fn()}
          onOpenSetupTab={vi.fn()}
        />,
      ),
    );
    const button = container.querySelector<HTMLButtonElement>(
      '[data-testid="workspace-header-new-session"]',
    );
    expect(button?.textContent).toBe("workspace.header.actions.newAgent");
    act(() => button!.click());
    expect(createSession).toHaveBeenCalledOnce();
    expect(createTerminal).not.toHaveBeenCalled();
    expect(openImport).not.toHaveBeenCalled();
  } finally {
    act(() => root.unmount());
  }
});
