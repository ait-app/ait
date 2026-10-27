/** @vitest-environment jsdom */
import React, { useEffect } from "react";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { TerminalEmulatorProps } from "./terminal-emulator-contract";
import { TerminalPane } from "./terminal-pane";

interface Scenario {
  connected: boolean;
  clientReady: boolean;
  serverInfo: { features: Record<string, boolean> } | null;
  settings: {
    useLegacyTerminalRenderer: boolean;
    monoFontFamily: string;
    terminalScrollbackLines: number;
    codeFontSize: number;
  };
  mounted: number;
  disposed: number;
}

const scenario = vi.hoisted<Scenario>(() => ({
  connected: false,
  clientReady: false,
  serverInfo: null,
  settings: {
    useLegacyTerminalRenderer: false,
    monoFontFamily: "",
    terminalScrollbackLines: 1000,
    codeFontSize: 14,
  },
  mounted: 0,
  disposed: 0,
}));

vi.mock("react-i18next", async (importOriginal) => ({
  ...(await importOriginal<typeof import("react-i18next")>()),
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock("@/hooks/use-settings", () => ({
  useAppSettings: () => ({ settings: scenario.settings }),
}));
vi.mock("@/runtime/host-runtime", () => ({
  useHostRuntimeClient: () => (scenario.clientReady ? {} : null),
  useHostRuntimeIsConnected: () => scenario.connected,
}));
vi.mock("@/stores/session-store", () => ({
  useSessionStore: <T,>(
    select: (state: {
      sessions: Record<string, { serverInfo: Scenario["serverInfo"] }>;
      setFocusedTerminalId: () => void;
    }) => T,
  ) =>
    select({
      sessions: { "test-host": { serverInfo: scenario.serverInfo } },
      setFocusedTerminalId: vi.fn(),
    }),
}));
vi.mock("@/stores/panel-store", () => ({
  usePanelStore: <T,>(
    select: (state: { mobilePanel: { target: string }; showMobileAgentList: () => void }) => T,
  ) => select({ mobilePanel: { target: "agent" }, showMobileAgentList: vi.fn() }),
}));
vi.mock("@/constants/platform", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/constants/platform")>()),
  isNative: true,
}));
vi.mock("@/constants/layout", () => ({ useIsCompactFormFactor: () => false }));
vi.mock("@/keyboard/shift", () => ({
  useKeyboardShiftStyle: () => ({ shift: { value: 0 }, style: {} }),
}));
vi.mock("@/hooks/use-app-visible", () => ({ useAppActivelyVisible: () => false }));
vi.mock("@/components/retained-panel", () => ({ useRetainedPanelActive: () => true }));
vi.mock("@/mobile-panels/provider", () => ({ useBlockMobilePanelOpenGestures: vi.fn() }));
vi.mock("react-native-reanimated", async () => ({
  default: { View: (await import("react-native")).View },
  useAnimatedReaction: vi.fn(),
  runOnJS: <T,>(callback: T) => callback,
}));
vi.mock("@/terminal/find", () => ({ TerminalFind: () => null }));
vi.mock("lucide-react-native", () => ({ Keyboard: () => null, KeyboardOff: () => null }));
vi.mock("@/components/ui/loading-spinner", () => ({ LoadingSpinner: () => null }));
vi.mock("./terminal-copy-paste-actions", () => ({
  TerminalFloatingCopyAction: () => null,
  TerminalPasteAction: () => null,
}));
vi.mock("@/terminal/runtime/terminal-stream-controller", () => ({
  TerminalStreamController: class {
    dispose = vi.fn();
    setTerminal = vi.fn();
  },
}));
vi.mock("./terminal-emulator", () => import("./terminal-emulator.native"));
vi.mock("./terminal-emulator-native-grid.native", () => ({
  default: function NativeGridTerminal({
    streamKey,
    onRendererReadyChange,
  }: TerminalEmulatorProps) {
    useEffect(() => {
      scenario.mounted += 1;
      return () => {
        scenario.disposed += 1;
      };
    }, []);
    useEffect(() => {
      onRendererReadyChange?.({ streamKey, isReady: true });
    }, [onRendererReadyChange, streamKey]);
    return <div data-testid="native-grid-terminal">cached terminal output</div>;
  },
}));
vi.mock("./terminal-emulator-webview.native", () => ({
  default: () => <div data-testid="webview-terminal" />,
}));

function pane() {
  return (
    <TerminalPane
      serverId="test-host"
      cwd="/qa"
      terminalId="terminal-1"
      isWorkspaceFocused
      isPaneFocused={false}
      onOpenFileExplorer={vi.fn()}
      onOpenWorkspaceFile={vi.fn()}
    />
  );
}

function connect(features: Record<string, boolean> | null) {
  scenario.connected = true;
  scenario.clientReady = true;
  scenario.serverInfo = features === null ? null : { features };
}

beforeEach(() => {
  scenario.connected = false;
  scenario.clientReady = false;
  scenario.serverInfo = null;
  scenario.settings.useLegacyTerminalRenderer = false;
  scenario.mounted = 0;
  scenario.disposed = 0;
  vi.clearAllMocks();
});
afterEach(cleanup);

describe("native terminal status messages", () => {
  it("only shows the disconnected message before host capabilities are known", () => {
    render(pane());
    expect(screen.getByText("workspace.terminal.hostDisconnected")).toBeTruthy();
    expect(screen.queryByText("workspace.terminal.updateHost")).toBeNull();
    expect(screen.queryByTestId("terminal-attach-loading")).toBeNull();
  });

  it("does not show an upgrade message from cached capabilities while disconnected", () => {
    scenario.serverInfo = { features: {} };
    render(pane());
    expect(screen.getByText("workspace.terminal.hostDisconnected")).toBeTruthy();
    expect(screen.queryByText("workspace.terminal.updateHost")).toBeNull();
  });

  it("waits for host capabilities after connecting before suggesting an upgrade", () => {
    connect(null);
    render(pane());
    expect(screen.queryByText("workspace.terminal.updateHost")).toBeNull();
    expect(screen.getByTestId("terminal-attach-loading")).toBeTruthy();
  });

  it("only shows the upgrade message for a connected unsupported host", () => {
    connect({});
    render(pane());
    expect(screen.getByText("workspace.terminal.updateHost")).toBeTruthy();
    expect(screen.queryByTestId("terminal-host-disconnected")).toBeNull();
    expect(screen.queryByTestId("terminal-attach-loading")).toBeNull();
  });

  it("replaces the loading state with the upgrade message when capabilities arrive", () => {
    connect(null);
    const view = render(pane());
    scenario.serverInfo = { features: {} };
    view.rerender(pane());
    expect(screen.getByText("workspace.terminal.updateHost")).toBeTruthy();
    expect(screen.queryByTestId("terminal-attach-loading")).toBeNull();
  });

  it("starts the native renderer when a supported host finishes reporting capabilities", () => {
    connect(null);
    const view = render(pane());
    scenario.serverInfo = { features: { "terminal-input-mode-replay": true } };
    view.rerender(pane());
    expect(screen.getByTestId("native-grid-terminal")).toBeTruthy();
    expect(screen.queryByText("workspace.terminal.updateHost")).toBeNull();
    expect(screen.queryByTestId("terminal-attach-loading")).toBeNull();
  });

  it("keeps the native renderer and cached output mounted through a reconnect", () => {
    connect({ "terminal-input-mode-replay": true });
    const view = render(pane());
    const terminal = screen.getByTestId("native-grid-terminal");
    scenario.connected = false;
    view.rerender(pane());
    expect(screen.getByText("workspace.terminal.hostDisconnected")).toBeTruthy();
    expect(screen.getByTestId("native-grid-terminal")).toBe(terminal);
    scenario.connected = true;
    view.rerender(pane());
    expect(screen.queryByTestId("terminal-host-disconnected")).toBeNull();
    expect(screen.getByTestId("native-grid-terminal")).toBe(terminal);
    expect(scenario.mounted).toBe(1);
    expect(scenario.disposed).toBe(0);
  });

  it("keeps the explicitly selected legacy renderer available on older hosts", () => {
    connect({});
    scenario.settings.useLegacyTerminalRenderer = true;
    render(pane());
    expect(screen.getByTestId("webview-terminal")).toBeTruthy();
    expect(screen.queryByText("workspace.terminal.updateHost")).toBeNull();
  });
});
