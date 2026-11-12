/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { usePanelStore } from "@/stores/panel-store";
import { useToggleAgentList } from "./use-toggle-agent-list";

vi.mock("react-native", () => ({ useWindowDimensions: () => ({ width: 852 }) }));
vi.mock("@/constants/platform", () => ({ isNative: true, isWeb: false }));
vi.mock("@/stores/navigation-active-workspace-store", () => ({
  useActiveWorkspaceSelection: () => ({ serverId: "host", workspaceId: "workspace" }),
}));

// The geometry helper needs only this exported minimum, not explorer UI controls.
vi.mock("@/components/explorer-sidebar-layout", () => ({ MIN_EXPLORER_SIDEBAR_WIDTH: 240 }));
vi.mock("@/constants/layout", () => ({ SETTINGS_DESKTOP_SPLIT_MIN_WIDTH: 720 }));

afterEach(() => {
  cleanup();
  usePanelStore.setState(usePanelStore.getInitialState());
});

describe("native navigation toggle", () => {
  it.each([true, false])(
    "opens yielded navigation regardless of stored preference %s",
    (agentListOpen) => {
      usePanelStore.setState({
        desktop: { agentListOpen, focusModeEnabled: false },
        mobilePanel: { target: "file-explorer", revision: 4 },
      });
      const { result } = renderHook(() => useToggleAgentList(false));
      act(() => result.current());
      expect(usePanelStore.getState().desktop.agentListOpen).toBe(true);
      expect(usePanelStore.getState().mobilePanel).toEqual({ target: "agent", revision: 5 });
      act(() => result.current());
      expect(usePanelStore.getState().desktop.agentListOpen).toBe(false);
    },
  );

  it("keeps compact navigation in the existing mobile panel model", () => {
    usePanelStore.setState({ mobilePanel: { target: "file-explorer", revision: 4 } });
    const { result } = renderHook(() => useToggleAgentList(true));
    act(() => result.current());
    expect(usePanelStore.getState().mobilePanel).toEqual({ target: "agent-list", revision: 5 });
  });
});
